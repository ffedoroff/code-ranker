//! # code-split-plugin-api
//!
//! The contract everything in Code Split builds on: a **generic property-graph
//! model** plus the [`LanguagePlugin`] trait. This crate is the foundation — it
//! depends on **nothing** else from Code Split and re-exports nothing. Every
//! other crate (graph operations, complexity, language plugins, viewer, cli)
//! depends on *this*.
//!
//! ## Model
//!
//! Analysis produces a [`Graph`] of **[`Node`]**s connected by **[`Edge`]**s.
//! A node is *anything we analyze*: today a source file (`kind == "file"`),
//! tomorrow a folder, module, function, variable or line — with **no model
//! change**. `kind` is a free-form [`String`] (the plugin's own vocabulary);
//! the core never interprets it, it only stores and projects.
//!
//! Both nodes and edges carry free-form **[`Attributes`]** (string key →
//! scalar [`AttrValue`]). There is no fixed, file/language-specific field set:
//! the plugin chooses keys (`path`, `loc`, `visibility`, `version`, or
//! language-specific ones), the orchestrator adds computed keys (metrics,
//! cycle), and the core reads only the keys it understands. Each level describes
//! its keys with an [`AttributeSpec`] dictionary (type + label/hint), so the UI
//! knows what each key means and what it can do with it.
//!
//! ## Responsibilities
//!
//! A [`LanguagePlugin`] is a **pure parser**: it turns a workspace into nodes +
//! edges at a requested level (by name; see [`Level`]). It does **not**
//! compute metrics — complexity / cycles / Henry-Kafura / stats are filled in
//! centrally, for all languages, by the orchestrator. The plugin also describes
//! its edge kinds ([`EdgeKindSpec`]) and attribute keys
//! ([`AttributeSpec`]), so the core scores, draws and labels unknown
//! kinds/keys without hardcoding their names.

pub mod attrs;
pub mod edge;
pub mod graph;
pub mod level;
pub mod node;
pub mod plugin;

pub use attrs::{AttrValue, Attributes, ValueType};
pub use edge::Edge;
pub use graph::Graph;
pub use level::{AttributeGroup, AttributeSpec, EdgeKindSpec, Level};
pub use node::{Node, NodeId};
pub use plugin::{LanguagePlugin, Options, PluginInput};
