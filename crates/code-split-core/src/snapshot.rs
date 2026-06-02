use crate::graph::{Graph, NodeKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTime {
    pub stage: String,
    pub ms: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: String,
    pub generated_at: DateTime<Utc>,
    pub command: String,
    /// Directory from which `code-split` was invoked.
    pub workspace: String,
    /// The analyzed project directory (absolute path, stored once here).
    pub target: String,
    pub plugin: String,
    /// Config file used for this analysis, if any was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_file: Option<String>,
    pub versions: HashMap<String, String>,
    /// Named system roots used to shorten node paths (e.g. `{cargo}`, `{rustup}`).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub roots: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
    /// Per-stage timing in milliseconds, in execution order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timings: Vec<StageTime>,
    pub graphs: PluginGraphs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    pub branch: String,
    pub commit: String,
    pub dirty_files: u32,
    /// Remote `origin` URL (raw, e.g. `git@gitlab.example.com:group/proj.git`).
    /// Used by the HTML report to build "open in GitLab/GitHub" source links.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginGraphs {
    /// The single file-level graph: `File` nodes + `External` library nodes,
    /// connected by file→file `Uses`/`Reexports` edges and file→library
    /// `Uses {external}` edges.
    pub files: Graph,
}

impl Snapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command: String,
        workspace: String,
        target: String,
        plugin: String,
        config_file: Option<String>,
        versions: HashMap<String, String>,
        roots: HashMap<String, String>,
        git: Option<GitInfo>,
        timings: Vec<StageTime>,
        graphs: PluginGraphs,
    ) -> Self {
        Self {
            schema_version: "1".to_string(),
            generated_at: Utc::now(),
            command,
            workspace,
            target,
            plugin,
            config_file,
            versions,
            roots,
            git,
            timings,
            graphs,
        }
    }
}

// ---------------------------------------------------------------------------
// Canonical (deterministic) JSON serialization
// ---------------------------------------------------------------------------

/// Serialize to canonical pretty JSON: every object key is emitted in
/// alphabetical order and the graph `nodes` / `edges` arrays are sorted by a
/// stable key (`id` for nodes; `from`, `to`, `kind` for edges). This makes the
/// output byte-stable for unchanged input — re-running the analysis never
/// reorders keys (e.g. from `HashMap` iteration order) or array entries.
///
/// `serde_json::Value` is backed by a `BTreeMap`, so round-tripping through it
/// yields alphabetical keys for free; we only sort the data arrays explicitly.
pub fn to_canonical_string_pretty<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let mut v = serde_json::to_value(value)?;
    canonicalize_value(&mut v);
    serde_json::to_string_pretty(&v)
}

/// Compact counterpart of [`to_canonical_string_pretty`] (no indentation).
pub fn to_canonical_string<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let mut v = serde_json::to_value(value)?;
    canonicalize_value(&mut v);
    serde_json::to_string(&v)
}

fn canonicalize_value(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                canonicalize_value(item);
            }
        }
        serde_json::Value::Object(map) => {
            for val in map.values_mut() {
                canonicalize_value(val);
            }
            // Data arrays get a stable order so two snapshots of unchanged code
            // are byte-identical regardless of plugin emission order.
            if let Some(serde_json::Value::Array(nodes)) = map.get_mut("nodes") {
                nodes.sort_by_key(|a| json_str(a, "id"));
            }
            if let Some(serde_json::Value::Array(edges)) = map.get_mut("edges") {
                edges.sort_by(|a, b| {
                    json_str(a, "from")
                        .cmp(&json_str(b, "from"))
                        .then_with(|| json_str(a, "to").cmp(&json_str(b, "to")))
                        .then_with(|| json_str(a, "kind").cmp(&json_str(b, "kind")))
                });
            }
        }
        _ => {}
    }
}

fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

// ---------------------------------------------------------------------------
// Path relativization
// ---------------------------------------------------------------------------

/// Rewrite all node `path` fields to be relative:
/// - paths under `target` → plain relative path (`src/main.rs`)
/// - paths under a named root → `{name}/rest/of/path`
/// - anything else → left as-is
pub fn relativize_graphs(
    graphs: &mut PluginGraphs,
    target: &Path,
    roots: &HashMap<String, String>,
) {
    for node in &mut graphs.files.nodes {
        node.path = relativize_path(&node.path, target, roots);
    }
}

pub(crate) fn relativize_path(
    path: &str,
    target: &Path,
    roots: &HashMap<String, String>,
) -> String {
    if path.is_empty() {
        return path.to_string();
    }
    let p = Path::new(path);
    // target first — local paths become {target}/src/...
    if let Ok(rel) = p.strip_prefix(target) {
        return format!("{{target}}/{}", rel.to_string_lossy());
    }
    // Longest root wins.
    let mut sorted: Vec<_> = roots.iter().collect();
    sorted.sort_by_key(|(_, root)| Reverse(root.len()));
    for (name, root) in &sorted {
        if let Ok(rel) = p.strip_prefix(root.as_str()) {
            return format!("{{{name}}}/{}", rel.to_string_lossy());
        }
    }
    path.to_string()
}

// ---------------------------------------------------------------------------
// ID rewriting  (Variant A: {crate_name}::{path}, version only on conflict)
// ---------------------------------------------------------------------------

/// Rewrite all node `id`, `parent` and edge `from`/`to` fields from the raw
/// cargo-based IDs to short human-readable IDs.
///
/// Scheme:
/// - `crate:{pkg_repr}` → `crate:anyhow` / `crate:anyhow@1.0.102` (conflict)
/// - `mod:{pkg_repr}::{path}` → `mod:anyhow::{path}`
/// - `trait:{pkg_repr}::{path}` → `trait:anyhow::{path}`
/// - `file:{abs_path}` → `file:{rel_path}` (relativized via roots)
pub fn rewrite_ids(graphs: &mut PluginGraphs, target: &Path, roots: &HashMap<String, String>) {
    // Step 1: collect (pkg_repr → (name, version)) from crate nodes.
    let mut pkg_info: HashMap<String, (String, String)> = HashMap::new();
    for node in graphs.files.nodes.iter() {
        if node.kind == NodeKind::Crate
            && let Some(pkg_repr) = node.id.strip_prefix("crate:")
        {
            pkg_info
                .entry(pkg_repr.to_string())
                .or_insert_with(|| parse_pkg_repr(pkg_repr));
        }
    }

    // Step 2: detect name conflicts (same name, different versions).
    let mut name_versions: HashMap<String, HashSet<String>> = HashMap::new();
    for (name, version) in pkg_info.values() {
        name_versions
            .entry(name.clone())
            .or_default()
            .insert(version.clone());
    }

    // Step 3: pkg_repr → short crate identifier.
    let crate_map: HashMap<String, String> = pkg_info
        .iter()
        .map(|(repr, (name, version))| {
            let conflict = name_versions.get(name).is_some_and(|v| v.len() > 1);
            let short = if conflict && !version.is_empty() {
                format!("{name}@{version}")
            } else {
                name.clone()
            };
            (repr.clone(), short)
        })
        .collect();

    // Step 4: build old_id → new_id for every node across all graphs.
    let mut id_map: HashMap<String, String> = HashMap::new();
    for node in graphs.files.nodes.iter() {
        let new_id = rewrite_node_id(&node.id, &crate_map, target, roots);
        if new_id != node.id {
            id_map.insert(node.id.clone(), new_id);
        }
    }

    // Step 5: apply mapping to nodes and edges.
    let graph = &mut graphs.files;
    for node in &mut graph.nodes {
        if let Some(new_id) = id_map.get(&node.id) {
            node.id = new_id.clone();
        }
        if let Some(parent) = node.parent.as_mut() {
            if let Some(new_parent) = id_map.get(parent.as_str()) {
                *parent = new_parent.clone();
            } else {
                // parent references a node not in any graph (e.g. stdlib file node);
                // rewrite it directly instead of relying on id_map lookup.
                let rewritten = rewrite_node_id(parent, &crate_map, target, roots);
                if rewritten != *parent {
                    *parent = rewritten;
                }
            }
        }
    }
    for edge in &mut graph.edges {
        if let Some(v) = id_map.get(&edge.from) {
            edge.from = v.clone();
        }
        if let Some(v) = id_map.get(&edge.to) {
            edge.to = v.clone();
        }
    }
}

fn rewrite_node_id(
    id: &str,
    crate_map: &HashMap<String, String>,
    target: &Path,
    roots: &HashMap<String, String>,
) -> String {
    // crate:
    if let Some(pkg_repr) = id.strip_prefix("crate:") {
        let short = crate_map
            .get(pkg_repr)
            .cloned()
            .unwrap_or_else(|| parse_pkg_repr(pkg_repr).0);
        return format!("crate:{short}");
    }
    // mod: / trait: / fn: / method:
    for kind in ["mod", "trait", "fn", "method"] {
        let prefix = format!("{kind}:");
        if let Some(rest) = id.strip_prefix(&prefix)
            && let Some((pkg_repr, path_part)) = split_version_boundary(rest)
        {
            let short = crate_map
                .get(&pkg_repr)
                .cloned()
                .unwrap_or_else(|| parse_pkg_repr(&pkg_repr).0);
            // Strip redundant `{crate_name}::` prefix from path when target == crate.
            let trimmed = path_part
                .strip_prefix(&format!("{short}::"))
                .unwrap_or(&path_part)
                .to_string();
            return format!("{kind}:{short}::{trimmed}");
        }
    }
    // file:
    if let Some(abs_path) = id.strip_prefix("file:") {
        let rel = relativize_path(abs_path, target, roots);
        return format!("file:{rel}");
    }
    id.to_string()
}

/// Split `path+file:///path#0.1.0::mod::sub` into
/// (`path+file:///path#0.1.0`, `mod::sub`).
fn split_version_boundary(s: &str) -> Option<(String, String)> {
    let hash_pos = s.find('#')?;
    let after_hash = &s[hash_pos + 1..];
    let colon_pos = after_hash.find("::")?;
    let pkg_repr = s[..hash_pos + 1 + colon_pos].to_string();
    let path_part = after_hash[colon_pos + 2..].to_string();
    Some((pkg_repr, path_part))
}

/// Extract `(name, version)` from a raw cargo package repr.
///
/// Examples:
/// - `path+file:///path/to/anyhow#1.0.102`   → (`anyhow`, `1.0.102`)
/// - `registry+https://...#anyhow@1.0.102`   → (`anyhow`, `1.0.102`)
/// - `git+https://...?tag=v0.1.0#a3f9c21`    → (`repo-name`, `v0.1.0`)
fn parse_pkg_repr(repr: &str) -> (String, String) {
    if let Some(hash_pos) = repr.rfind('#') {
        let after = &repr[hash_pos + 1..];
        // registry style: name@version after #
        if let Some((name, ver)) = after.split_once('@') {
            return (name.to_string(), ver.to_string());
        }
        // path style: version is after #, name is last component before #
        let version = after.to_string();
        let before = &repr[..hash_pos];
        // strip query string (?tag=...) if present
        let before = before.split('?').next().unwrap_or(before);
        let name = before
            .split('/')
            .next_back()
            .unwrap_or("unknown")
            .to_string();
        return (name, version);
    }
    // Fallback: use the whole thing as name
    (repr.to_string(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, EdgeKind, Node};

    fn node(id: &str, kind: NodeKind) -> Node {
        Node {
            id: id.into(),
            kind,
            name: id.into(),
            path: String::new(),
            parent: None,
            external: None,
            version: None,
            visibility: None,
            loc: None,
            line: None,
            item_count: None,
            method_count: None,
            complexity: None,
            cycle_kind: None,
        }
    }

    // ── serde round-trip of the public artifact (P1) ────────────────────────

    fn sample_snapshot() -> Snapshot {
        let mut graphs = PluginGraphs::default();
        graphs.files.nodes.push(node("crate:foo", NodeKind::Crate));
        Snapshot::new(
            "report".into(),
            "/work".into(),
            "/work/foo".into(),
            "rust".into(),
            None,
            HashMap::new(),
            HashMap::new(),
            None,
            Vec::new(),
            graphs,
        )
    }

    #[test]
    fn snapshot_roundtrips_through_json() {
        let snap = sample_snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        let back: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.schema_version, "1");
        assert_eq!(back.command, "report");
        assert_eq!(back.plugin, "rust");
        assert_eq!(back.target, "/work/foo");
        assert_eq!(back.graphs.files.nodes.len(), 1);
        assert_eq!(back.graphs.files.nodes[0].id, "crate:foo");
        // generated_at survives the RFC3339 round-trip to the same instant.
        assert_eq!(back.generated_at, snap.generated_at);
    }

    #[test]
    fn snapshot_omits_absent_optional_fields() {
        let json = serde_json::to_string(&sample_snapshot()).unwrap();
        assert!(
            !json.contains("\"git\""),
            "None git is not serialized: {json}"
        );
        assert!(!json.contains("config_file"), "None config_file is skipped");
        assert!(!json.contains("timings"), "empty timings is skipped");
    }

    #[test]
    fn snapshot_keeps_present_optional_fields() {
        let mut snap = sample_snapshot();
        snap.git = Some(GitInfo {
            branch: "main".into(),
            commit: "abc".into(),
            dirty_files: 2,
            origin: None,
        });
        snap.timings.push(StageTime {
            stage: "parse".into(),
            ms: 5,
            detail: String::new(),
        });
        let json = serde_json::to_string(&snap).unwrap();
        let back: Snapshot = serde_json::from_str(&json).unwrap();
        let git = back.git.unwrap();
        assert_eq!(git.branch, "main");
        assert_eq!(git.dirty_files, 2);
        assert_eq!(back.timings.len(), 1);
        assert_eq!(back.timings[0].stage, "parse");
    }

    // ── relativize_path ─────────────────────────────────────────────────────

    #[test]
    fn relativize_path_empty_stays_empty() {
        assert_eq!(relativize_path("", Path::new("/p"), &HashMap::new()), "");
    }

    #[test]
    fn relativize_path_under_target_uses_target_token() {
        let got = relativize_path("/p/src/main.rs", Path::new("/p"), &HashMap::new());
        assert_eq!(got, "{target}/src/main.rs");
    }

    #[test]
    fn relativize_path_under_named_root_uses_root_token() {
        let roots = HashMap::from([("cargo".to_string(), "/home/u/.cargo".to_string())]);
        let got = relativize_path("/home/u/.cargo/registry/foo.rs", Path::new("/p"), &roots);
        assert_eq!(got, "{cargo}/registry/foo.rs");
    }

    #[test]
    fn relativize_path_longest_root_wins() {
        // Both roots are prefixes of the path; the longer one (`cargo`) wins.
        let roots = HashMap::from([
            ("home".to_string(), "/home/u".to_string()),
            ("cargo".to_string(), "/home/u/.cargo".to_string()),
        ]);
        let got = relativize_path("/home/u/.cargo/x.rs", Path::new("/p"), &roots);
        assert_eq!(got, "{cargo}/x.rs");
    }

    #[test]
    fn relativize_path_unmatched_is_unchanged() {
        let got = relativize_path("/elsewhere/x.rs", Path::new("/p"), &HashMap::new());
        assert_eq!(got, "/elsewhere/x.rs");
    }

    // ── parse_pkg_repr ──────────────────────────────────────────────────────

    #[test]
    fn parse_pkg_repr_registry_path_and_fallback() {
        let cases = vec![
            (
                "registry+https://github.com/rust-lang/crates.io-index#anyhow@1.0.102",
                ("anyhow", "1.0.102"),
            ),
            ("path+file:///path/to/anyhow#1.0.102", ("anyhow", "1.0.102")),
            ("bare-name-no-hash", ("bare-name-no-hash", "")),
        ];
        for (repr, (name, ver)) in cases {
            let (gn, gv) = parse_pkg_repr(repr);
            assert_eq!(gn, name, "name for {repr:?}");
            assert_eq!(gv, ver, "version for {repr:?}");
        }
    }

    #[test]
    fn parse_pkg_repr_git_uses_commit_after_hash() {
        // Cargo git source ids carry the commit sha after `#`; the `?tag=...`
        // query is stripped and the repo name is the last path segment.
        // NB: the returned "version" is the commit, not the tag.
        let (name, ver) = parse_pkg_repr("git+https://github.com/foo/repo?tag=v0.1.0#a3f9c21");
        assert_eq!(name, "repo");
        assert_eq!(ver, "a3f9c21");
    }

    // ── split_version_boundary ──────────────────────────────────────────────

    #[test]
    fn split_version_boundary_splits_after_hash_at_first_path_colons() {
        let got = split_version_boundary("path+file:///p#0.1.0::mod::sub");
        assert_eq!(
            got,
            Some(("path+file:///p#0.1.0".to_string(), "mod::sub".to_string()))
        );
    }

    #[test]
    fn split_version_boundary_none_without_hash_or_path_colons() {
        assert_eq!(split_version_boundary("mod::sub"), None, "no '#'");
        assert_eq!(
            split_version_boundary("has#hash-but-no-colons"),
            None,
            "'#' present but no '::' after it"
        );
    }

    // ── rewrite_ids ─────────────────────────────────────────────────────────

    #[test]
    fn rewrite_ids_shortens_single_crate_to_name() {
        let mut graphs = PluginGraphs::default();
        graphs
            .files
            .nodes
            .push(node("crate:path+file:///x/anyhow#1.0.102", NodeKind::Crate));
        rewrite_ids(&mut graphs, Path::new("/x"), &HashMap::new());
        assert_eq!(graphs.files.nodes[0].id, "crate:anyhow");
    }

    #[test]
    fn rewrite_ids_disambiguates_name_conflicts_with_version() {
        // Same crate name at two versions → both keep `@version` suffixes.
        let mut graphs = PluginGraphs::default();
        graphs
            .files
            .nodes
            .push(node("crate:path+file:///a/foo#1.0.0", NodeKind::Crate));
        graphs
            .files
            .nodes
            .push(node("crate:path+file:///b/foo#2.0.0", NodeKind::Crate));
        rewrite_ids(&mut graphs, Path::new("/x"), &HashMap::new());
        let ids: Vec<&str> = graphs.files.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"crate:foo@1.0.0"), "got {ids:?}");
        assert!(ids.contains(&"crate:foo@2.0.0"), "got {ids:?}");
    }

    #[test]
    fn rewrite_ids_rewrites_edge_endpoints_and_file_ids() {
        let mut graphs = PluginGraphs::default();
        graphs
            .files
            .nodes
            .push(node("crate:path+file:///x/anyhow#1.0.102", NodeKind::Crate));
        graphs
            .files
            .nodes
            .push(node("file:/x/src/lib.rs", NodeKind::File));
        graphs.files.edges.push(Edge {
            from: "crate:path+file:///x/anyhow#1.0.102".into(),
            to: "file:/x/src/lib.rs".into(),
            kind: EdgeKind::Contains,
            unresolved: None,
            external: None,
            visibility: None,
        });
        rewrite_ids(&mut graphs, Path::new("/x"), &HashMap::new());
        // file id is relativized against the target.
        assert_eq!(graphs.files.nodes[1].id, "file:{target}/src/lib.rs");
        // edge endpoints follow the node-id rewrite.
        assert_eq!(graphs.files.edges[0].from, "crate:anyhow");
        assert_eq!(graphs.files.edges[0].to, "file:{target}/src/lib.rs");
    }
}
