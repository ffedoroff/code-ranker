//! The `docs <lang> <subject>` command: print a reference doc to stdout. No
//! analysis. Docs are **per-language**, so the language comes FIRST (a registered
//! plugin name, or `base` for the language-agnostic catalog). Forms:
//!
//! - `docs` → list the project's detected languages + every documentable one;
//! - `docs <lang>` → that language's full subject catalog;
//! - `docs <lang> <subject>` → the doc for the subject.
//!
//! A `<subject>` given without a language errors and points at the per-language
//! form. `<subject>` is `ai` (the offline AI-agent playbook), `metrics` /
//! `principles` (an index of each), a `<category>` (`loc`, `complexity`, …), a
//! `<metric>` (its spec card + prose doc), or a `<principle>` id. Subjects match
//! separator/case-insensitively (`fan_in` = `Fan-in` = `FAN in`).
//!
//! Specs are built from the language plugin's level specs + the central catalog;
//! `base` uses the neutral built-in catalog. Project `[plugins.<lang>.principles]`
//! / `[plugins.<lang>.metrics]` are first-class subjects too.

use anyhow::{Result, bail};
use code_ranker_plugin_api::Principle;
use code_ranker_plugin_api::level::{AttributeGroup, AttributeSpec};
use code_ranker_plugin_api::plugin::PluginInput;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::config::{self, TemplatesConfig};
use crate::{plugin, templates};

/// Everything the `docs` subjects render, built from config + plugin with no graph.
struct DocSpecs {
    principles: Vec<Principle>,
    /// Metric/coupling specs by key (central catalog ⊕ plugin refinement ⊕ project
    /// `[metrics.<key>]`).
    node_attributes: BTreeMap<String, AttributeSpec>,
    /// Category (group) label/description by key.
    groups: BTreeMap<String, AttributeGroup>,
    templates: TemplatesConfig,
}

/// Print a per-language reference doc. The FIRST argument is the language (a
/// registered plugin or `base`); the optional second is the subject. Bare `docs`
/// lists the project's languages; `docs <lang>` prints that language's catalog.
pub(crate) fn run(
    language: Option<&str>,
    subject: Option<&str>,
    config_entries: &[String],
) -> Result<()> {
    let input = std::path::Path::new(".");
    let loaded = config::load(input, config_entries, &[], &[], &[]).ok();
    let cfg = loaded.map(|loaded| loaded.config);

    // Bare `docs`: list the project's detected languages + every documentable one.
    let Some(language) = language else {
        print!(
            "{}",
            templates::with_trailing_newline(language_listing(cfg.as_ref(), input))
        );
        return Ok(());
    };

    // Resolve an alias (`js` → `javascript`) to the canonical name so every lookup
    // below works on it; `base` and unknown tokens pass through unchanged.
    let language = plugin::to_canonical(language);
    let language = language.as_str();

    // The first argument is the language — a registered plugin or `base`. A value
    // that is not a language is almost always a subject typed without one (e.g.
    // `docs hk`, `docs ai`); point the user at the per-language form.
    if !is_known_language(language) {
        bail!(
            "`{language}` is not a language — docs are per-language, so the language comes first:\n  \
             code-ranker docs <lang> {language}\n{}",
            languages_hint(cfg.as_ref(), input)
        );
    }

    // `docs <lang> ai` → the offline AI-agent playbook.
    if subject.is_some_and(|s| templates::normalize_id(s) == "ai") {
        emit(templates::ai_doc(language)?, language);
        return Ok(());
    }

    let specs = build_specs(language, cfg);

    let Some(subject) = subject else {
        // `docs <lang>`: the full subject catalog for that language.
        emit(render_catalog(&specs, language, None), language);
        return Ok(());
    };

    // Every subject is matched on its normalized form (case/separator-insensitive),
    // so `fan_in`, `Fan-in`, and `FAN in` all resolve the same metric.
    let want = templates::normalize_id(subject);
    if want == "metrics" {
        emit(render_metrics_index(&specs, language), language);
    } else if want == "principles" {
        emit(render_principles_index(&specs, language), language);
    } else if let Some(cat) = category_key(&specs, subject) {
        emit(render_category(&specs, language, &cat), language);
    } else if let Some(p) = specs
        .principles
        .iter()
        .find(|p| templates::normalize_id(&p.id) == want)
    {
        emit(render_principle(&specs, &p.id)?, language);
    } else if let Some(key) = specs
        .node_attributes
        .keys()
        .find(|k| templates::normalize_id(k) == want)
    {
        emit(render_metric(&specs, key), language);
    } else {
        // Unknown subject: print the catalog so the caller sees every option, then
        // fail (non-zero) — it was a real lookup miss, not a help request.
        emit(render_catalog(&specs, language, Some(subject)), language);
        bail!("unknown docs subject {subject:?} for language {language:?} — see the list above");
    }
    Ok(())
}

fn emit(md: String, lang: &str) {
    print!(
        "{}",
        templates::with_trailing_newline(localize_lang(md, lang))
    );
}

/// Make instructional `<lang>` placeholders concrete in served per-language docs, so
/// commands print runnable as-is (`docs rust hk`, `--plugins rust`). `base` is the
/// language-agnostic catalog, so its generic `<lang>` stays a placeholder.
fn localize_lang(md: String, lang: &str) -> String {
    if lang == "base" {
        md
    } else {
        md.replace("<lang>", lang)
    }
}

/// `base` (the language-agnostic catalog) or any registered plugin name.
fn is_known_language(lang: &str) -> bool {
    lang == "base" || plugin::registry().iter().any(|p| p.name() == lang)
}

/// Languages auto-detected in `input` (best-effort; empty on any failure).
fn detected_languages(cfg: Option<&config::model::Config>, input: &Path) -> Vec<String> {
    let lang_overrides = cfg.map(|c| c.plugins.languages.clone()).unwrap_or_default();
    let eff_cfgs: BTreeMap<String, toml::Table> = plugin::registry()
        .iter()
        .map(|p| {
            let name = p.name().to_string();
            (
                name.clone(),
                plugin::effective_plugin_config(&name, &lang_overrides),
            )
        })
        .collect();
    plugin::detect_all(&eff_cfgs, input, &PluginInput::default())
}

/// One-line hint naming where to run a subject: the project's detected languages
/// (or every available one when none detected), plus `base`.
fn languages_hint(cfg: Option<&config::model::Config>, input: &Path) -> String {
    let detected = detected_languages(cfg, input);
    if detected.is_empty() {
        format!(
            "Available languages: {} (or `base` for the language-agnostic docs).",
            plugin::names_with_aliases()
        )
    } else {
        format!(
            "This project's languages: {} (or `base` for the language-agnostic docs).",
            detected.join(", ")
        )
    }
}

/// The bare-`docs` listing: the project's detected languages, the other languages
/// available for docs, and how to drill in.
fn language_listing(cfg: Option<&config::model::Config>, input: &Path) -> String {
    let detected = detected_languages(cfg, input);
    let mut all: Vec<&str> = plugin::registry().iter().map(|p| p.name()).collect();
    all.sort_unstable();

    let mut out = String::from("plugins (languages):\n");
    out.push_str(" - base — the language-agnostic catalog (shared defaults)\n");
    for name in &all {
        if detected.iter().any(|d| d == name) {
            out.push_str(&format!(" - {name} — detected in this project\n"));
        } else {
            out.push_str(&format!(" - {name}\n"));
        }
    }
    out.push('\n');
    out.push_str("Run:\n");
    out.push_str("    code-ranker docs <lang>            # the full subject catalog\n");
    out.push_str(
        "    code-ranker docs <lang> <subject>  # a metric / principle / category, or `ai`\n",
    );
    out
}

/// Build the doc specs strictly for one resolved `plugin_name`, no analysis. The
/// node-attribute dictionary is the plugin's own `files`-level specs (its
/// `[node_attributes.*]` — e.g. Rust's `unsafe` / `items`) layered with the central
/// complexity + coupling specs and the project's node-scope `[metrics.<key>]`;
/// principles are the plugin catalog overlaid with `[principles.<ID>]`. Config is
/// best-effort (a broken file degrades to the plugin's own specs).
fn build_specs(plugin_name: &str, cfg: Option<config::model::Config>) -> DocSpecs {
    // The plugin's effective config (static base ⊕ user `[languages.base]` /
    // `[languages.<lang>]`), so docs reflect the same per-language overrides analysis
    // would apply. With no config it degrades to the plugin's own static defaults.
    let lang_overrides = cfg
        .as_ref()
        .map(|c| c.plugins.languages.clone())
        .unwrap_or_default();
    let eff_cfg = plugin::effective_plugin_config(plugin_name, &lang_overrides);

    // The per-language orchestrator config (`[plugins.base]` ⊕ `[plugins.<lang>]`):
    // its `ignore` / `metrics` / `principles` feed the doc specs. Best-effort.
    let lc = cfg
        .as_ref()
        .and_then(|c| c.language_config(plugin_name).ok())
        .unwrap_or_default();

    // Central, language-neutral metric specs + their category groups, refined by
    // the active plugin (e.g. Rust's `#[cfg(test)]` LOC nuance).
    let (default_metric_specs, metric_groups) = code_ranker_graph::metric_specs();
    let (coupling_specs, coupling_groups) = code_ranker_graph::coupling_specs();
    let metric_specs = plugin::metric_specs(plugin_name, &eff_cfg, default_metric_specs);

    // The plugin's own structural attribute specs + category groups, taken from the
    // `files` level WITHOUT analysis — this is what surfaces language metrics like
    // Rust's `unsafe` that live in `[node_attributes.*]`, not the central catalog.
    let files_level = plugin::levels(plugin_name, &eff_cfg)
        .into_iter()
        .find(|l| l.name == "files");
    let mut node_attributes = files_level
        .as_ref()
        .map(|l| l.node_attributes.clone())
        .unwrap_or_default();
    node_attributes.extend(metric_specs);
    node_attributes.extend(coupling_specs);

    let mut groups = files_level.map(|l| l.attribute_groups).unwrap_or_default();
    groups.extend(metric_groups);
    groups.extend(coupling_groups);

    let pinput = if cfg.is_some() {
        PluginInput {
            ignore: lc.ignore.paths.clone(),
            ignore_tests: lc.ignore.tests,
            gitignore: lc.ignore.gitignore,
            ignore_files: lc.ignore.ignore_files,
            hidden: lc.ignore.hidden,
        }
    } else {
        default_plugin_input()
    };

    // Project node-scope declarative metrics (built-ins win a key collision).
    for (k, d) in &lc.metrics {
        if d.scope == code_ranker_graph::Scope::Node {
            node_attributes
                .entry(k.clone())
                .or_insert_with(|| d.to_attribute_spec());
        }
    }

    // Principles: the plugin catalog overlaid with the language's `[principles.<ID>]`.
    // `base` is the language-agnostic catalog (not a registered plugin), so its
    // principles come from the neutral built-in defaults.
    let catalog = if plugin_name == "base" {
        code_ranker_plugins::config::resolved_principles(&code_ranker_plugins::config::load_chain(
            &[],
        ))
    } else {
        plugin::principles(plugin_name, &eff_cfg, &pinput)
    };
    let principles = config::merge_project_principles(catalog, &lc.principles);

    let templates = cfg.map(|c| c.templates).unwrap_or_default();

    DocSpecs {
        principles,
        node_attributes,
        groups,
        templates,
    }
}

/// A neutral `PluginInput` for the no-config fallback (a broken config file). The
/// principle/metric-spec hooks barely read these, so the defaults only affect the
/// rare degraded path.
fn default_plugin_input() -> PluginInput {
    PluginInput {
        ignore: Vec::new(),
        ignore_tests: true,
        gitignore: true,
        ignore_files: true,
        hidden: false,
    }
}

// ── subject resolution helpers ────────────────────────────────────────────────

/// Every category key: the defined groups plus any `group` a metric spec references
/// (a metric may name a category that ships no `[categories.<key>]` label).
fn category_keys(specs: &DocSpecs) -> BTreeSet<String> {
    let mut keys: BTreeSet<String> = specs.groups.keys().cloned().collect();
    for spec in specs.node_attributes.values() {
        if let Some(g) = &spec.group {
            keys.insert(g.clone());
        }
    }
    keys
}

/// The canonical category key matching `subject` (separator/case-insensitive), if any.
fn category_key(specs: &DocSpecs, subject: &str) -> Option<String> {
    let want = templates::normalize_id(subject);
    category_keys(specs)
        .into_iter()
        .find(|k| templates::normalize_id(k) == want)
}

/// The metrics in one category, by key (sorted — `BTreeMap` order).
fn metrics_in_category<'a>(specs: &'a DocSpecs, key: &str) -> Vec<(&'a String, &'a AttributeSpec)> {
    specs
        .node_attributes
        .iter()
        .filter(|(_, s)| s.group.as_deref() == Some(key))
        .collect()
}

// ── rendering ─────────────────────────────────────────────────────────────────

/// A metric's display name: `name` › `label` › the key itself.
fn metric_name<'a>(spec: &'a AttributeSpec, key: &'a str) -> &'a str {
    spec.name
        .as_deref()
        .or(spec.label.as_deref())
        .unwrap_or(key)
}

/// The first line of a (possibly multi-line, `<br>`-encoded) description.
fn one_line(desc: &str) -> &str {
    desc.split("<br>").next().unwrap_or(desc).trim()
}

/// A category's label (› its key) and optional description.
fn category_label(specs: &DocSpecs, key: &str) -> String {
    specs
        .groups
        .get(key)
        .and_then(|g| g.label.clone())
        .unwrap_or_else(|| key.to_string())
}

/// Strip a leading `ID — ` from a principle title so the listing column is tight.
fn principle_title(p: &Principle) -> &str {
    p.title
        .split_once(" — ")
        .map(|(_, rest)| rest)
        .unwrap_or(&p.title)
}

/// The categories section shared by `docs metrics` and the catalog: each category
/// header (`key: Label — description`) followed by its member metrics.
fn categories_block(specs: &DocSpecs) -> String {
    let mut out = String::new();
    let cats = category_keys(specs);
    for key in &cats {
        // Header is `<key> — <description>`: the key is what you type (`docs <key>`),
        // the description says what the category measures. The Titlecase `label` is
        // dropped here — it just echoes the key (`complexity` ≈ "Complexity").
        out.push_str(&format!("\n  {key}"));
        match specs.groups.get(key).and_then(|g| g.description.as_deref()) {
            Some(d) => out.push_str(&format!(" — {d}")),
            None => out.push_str(&format!(" — {}", category_label(specs, key))),
        }
        out.push('\n');
        for (k, spec) in metrics_in_category(specs, key) {
            out.push_str(&format!("    - {k}: {}\n", metric_name(spec, k)));
        }
    }
    // Metrics with no category (e.g. the categorical `cycle`, Rust's `unsafe`): list
    // them too — but only those with a description (skips bare external-node metadata
    // like `crate` / `version` that carry no doc copy).
    let uncategorized: Vec<_> = specs
        .node_attributes
        .iter()
        .filter(|(_, s)| s.group.is_none() && s.description.is_some())
        .collect();
    if !uncategorized.is_empty() {
        out.push_str("\n  (uncategorized)\n");
        for (k, spec) in uncategorized {
            out.push_str(&format!("    - {k}: {}\n", metric_name(spec, k)));
        }
    }
    out
}

/// The principles section shared by `docs principles` and the catalog.
fn principles_block(specs: &DocSpecs) -> String {
    if specs.principles.is_empty() {
        return "  (none — this plugin defines no principles)\n".to_string();
    }
    specs
        .principles
        .iter()
        .map(|p| format!("  - {}: {}\n", p.id, principle_title(p)))
        .collect()
}

/// `docs <lang> metrics`: every metric, grouped by category.
fn render_metrics_index(specs: &DocSpecs, lang: &str) -> String {
    format!(
        "Metrics — print one with `code-ranker docs {lang} <metric>`:\n{}",
        categories_block(specs)
    )
}

/// `docs <lang> principles`: every design principle.
fn render_principles_index(specs: &DocSpecs, lang: &str) -> String {
    format!(
        "Principles — print one with `code-ranker docs {lang} <ID>`:\n\n{}",
        principles_block(specs)
    )
}

/// `docs <lang> <category>`: the category's human label + description + its member metrics.
fn render_category(specs: &DocSpecs, lang: &str, key: &str) -> String {
    // Single-category view: the human label is the title (the key was just typed),
    // so there is no `key: Label` echo.
    let mut out = category_label(specs, key);
    if let Some(d) = specs.groups.get(key).and_then(|g| g.description.as_deref()) {
        out.push_str(&format!("\n{d}"));
    }
    out.push_str(&format!(
        "\n\nMetrics — print one with `code-ranker docs {lang} <metric>`:\n"
    ));
    for (k, spec) in metrics_in_category(specs, key) {
        out.push_str(&format!("  - {k}: {}", metric_name(spec, k)));
        if let Some(d) = spec.description.as_deref() {
            out.push_str(&format!(" — {}", one_line(d)));
        }
        out.push('\n');
    }
    out
}

/// `docs <metric>`: the spec card (label / name / category / description / formula),
/// then the full prose doc appended when one exists (e.g. `hk` → `HK.md`).
fn render_metric(specs: &DocSpecs, subject: &str) -> String {
    let (key, spec) = specs
        .node_attributes
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(subject))
        .expect("caller checked the key exists");
    let name = metric_name(spec, key);
    let mut out = format!("# {key}: {name}");
    if let Some(short) = spec.short.as_deref().filter(|s| *s != name) {
        out.push_str(&format!(" ({short})"));
    }
    out.push('\n');
    if let Some(g) = &spec.group {
        out.push_str(&format!("\nCategory: {g} — {}\n", category_label(specs, g)));
    }
    if let Some(d) = spec.description.as_deref() {
        out.push_str(&format!("\n{}\n", d.replace("<br>", "\n")));
    }
    if let Some(f) = &spec.formula {
        out.push_str(&format!("\nFormula: {f}\n"));
    }
    // A metric whose `remediation` points at a corpus doc (e.g. `hk` → `HK.md`)
    // gets that full doc appended — so `docs hk` is the complete reference.
    if let Ok(prose) = templates::resolve_doc_from_specs(
        &specs.principles,
        &specs.node_attributes,
        &specs.templates,
        key,
    ) {
        out.push_str(&format!("\n---\n\n{}\n", prose.trim_end()));
    }
    out
}

/// `docs <principle>`: the full prose doc, or — for a project-defined principle with
/// no doc file — a synthetic card from its title / sort-metric / prompt.
fn render_principle(specs: &DocSpecs, subject: &str) -> Result<String> {
    match templates::resolve_doc_from_specs(
        &specs.principles,
        &specs.node_attributes,
        &specs.templates,
        subject,
    ) {
        Ok(md) => Ok(md),
        Err(_) => {
            let p = specs
                .principles
                .iter()
                .find(|p| p.id.eq_ignore_ascii_case(subject))
                .expect("caller checked the principle exists");
            let mut out = format!(
                "# {}: {}\n\nSort metric: `{}`\n",
                p.id, p.title, p.sort_metric
            );
            if !p.prompt.is_empty() {
                out.push_str(&format!("\n{}\n", p.prompt));
            }
            Ok(out)
        }
    }
}

/// The catalog of every subject — shown for a bare `docs` (help) and, with a lead
/// note, for an unknown subject. A uniform two-level tree: each group (a metric
/// category, then `principles`) on its own line, its members indented beneath. Every
/// name on every line — group or member — is itself a valid `docs <subject>`.
fn render_catalog(specs: &DocSpecs, lang: &str, unknown: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(s) = unknown {
        out.push_str(&format!(
            "Unknown docs subject `{s}` for language `{lang}`.\n\n"
        ));
    }
    out.push_str(&format!(
        "code-ranker docs {lang} <subject> — print a reference doc to stdout (no analysis).\n"
    ));
    out.push_str(&categories_block(specs));
    // Principles render as one more group, exactly like a metric category.
    out.push_str("\n  principles — SOLID & related design principles\n");
    out.push_str(
        &specs
            .principles
            .iter()
            .map(|p| format!("    - {}: {}\n", p.id, principle_title(p)))
            .collect::<String>(),
    );
    out.push_str(&format!(
        "\nCall `docs {lang}` with any name above — e.g. `docs {lang} principles`, \
         `docs {lang} KISS`, `docs {lang} cloc`, `docs {lang} complexity`. Also \
         `docs {lang} ai` (the agent playbook) and `docs {lang} metrics` (the full metric index).\n"
    ));
    out
}

#[cfg(test)]
#[path = "docs_test.rs"]
mod tests;
