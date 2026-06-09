// grouping.js — the grouping ladder for the map's relative "dig" (level-of-detail).
//
// Two orthogonal navigation axes (see docs/code-split-viewer/REFACTOR-split-plan.md):
//   • window.dig  — relative LOD on the OVERVIEW.
//       dig  0  → every crate is its own node (the default).
//       dig +N  → dig IN: descend N directory levels inside crates (folder groups).
//       dig -N  → dig OUT: progressively collapse the DEEPEST crates into their
//                 parent folder, one depth level per step, until a single root
//                 group remains.
//   • focus (window.drillGroup) — click a group to drill into just its files.
//
// For Rust (group=crate, node=file) every tier is DERIVED from file-id paths plus
// the crate grouping attribute — no extra backend data. dig 0 reproduces the
// legacy per-crate grouping.

const DIG_MIN = -12, DIG_MAX = 6;
function clampDig(z) { return Math.max(DIG_MIN, Math.min(DIG_MAX, (z | 0))); }
window.clampDig = clampDig;

// Strip the leading `{token}/` root marker from an id/path.
function relPathOf(id) { return String(id || '').replace(/^\{[^}]+\}\//, ''); }

// Per-level memoised crate-root directories: the common directory prefix of all
// files sharing a crate value (used by the dig-IN branch to find dirs under a
// crate).
const _crateRootCache = new Map();   // level -> Map<crateValue, string[] dirSegs>
function crateRoots(level) {
  if (_crateRootCache.has(level)) return _crateRootCache.get(level);
  const gk = levelUi(level).grouping?.key;
  const byCrate = new Map();
  if (gk) {
    for (const n of (unionGraph(level).nodes || [])) {
      if (isExternalNode(n, level)) continue;
      const crate = n[gk];
      if (crate == null || crate === '') continue;
      const dirs = relPathOf(n.id).split('/').slice(0, -1);   // drop the filename
      const arr  = byCrate.get(String(crate));
      if (arr) arr.push(dirs); else byCrate.set(String(crate), [dirs]);
    }
  }
  const roots = new Map();
  for (const [crate, list] of byCrate) {
    let prefix = list[0].slice();
    for (let k = 1; k < list.length; k++) {
      const segs = list[k];
      let i = 0;
      while (i < prefix.length && i < segs.length && prefix[i] === segs[i]) i++;
      prefix = prefix.slice(0, i);
    }
    roots.set(crate, prefix);
  }
  _crateRootCache.set(level, roots);
  return roots;
}

// Per-level crate DIRECTORY paths (the crate-root with trailing source dirs like
// `src`/`tests` trimmed, so depth reflects where the crate sits in the workspace
// tree, not its internal layout) + the deepest such depth. Drives the dig-OUT
// progressive collapse.
const _crateDirsCache = new Map();   // level -> { dirOf: Map<crate,string[]>, maxDepth }
const SRC_DIRS = new Set(['src', 'tests', 'benches', 'lib', 'bin']);
function crateDirs(level) {
  if (_crateDirsCache.has(level)) return _crateDirsCache.get(level);
  const dirOf = new Map();
  let maxDepth = 0;
  for (const [crate, segs0] of crateRoots(level)) {
    const segs = segs0.slice();
    while (segs.length && SRC_DIRS.has(segs[segs.length - 1])) segs.pop();
    dirOf.set(crate, segs);
    if (segs.length > maxDepth) maxDepth = segs.length;
  }
  const res = { dirOf, maxDepth };
  _crateDirsCache.set(level, res);
  return res;
}
// The dig-OUT depth at which the overview collapses to a single root group.
function maxCrateDepth(level) { return crateDirs(level).maxDepth; }
window.maxCrateDepth = maxCrateDepth;

// Snapshot swaps change the node set → drop the memoised caches.
function clearGroupingCache() { _crateRootCache.clear(); _crateDirsCache.clear(); }
window.clearGroupingCache = clearGroupingCache;

// Group key for a node at a given dig level. dig 0 → the crate value (matches the
// legacy grouping); dig>0 appends directory segments under the crate; dig<0
// collapses the deepest crates into their ancestor folders, deepest first.
function groupKeyAtDig(level, n, dig) {
  if (isExternalNode(n, level))
    return (nodeKindSpec(level, n.kind).plural || 'external').toLowerCase();

  const d     = dig | 0;
  const gk    = levelUi(level).grouping?.key;
  const crate = gk ? n[gk] : null;
  const dirs  = relPathOf(n.id).split('/').slice(0, -1);

  // No crate attribute: plain directory tiers (dig 0 = full dir).
  if (crate == null || crate === '') {
    const depth = dirs.length + d;
    const keep  = dirs.slice(0, Math.max(0, depth));
    return keep.length ? keep.join('/') : '_root';
  }

  if (d >= 0) {
    // Dig IN: the crate, then folders under it.
    const root       = crateRoots(level).get(String(crate)) || [];
    const underCrate = dirs.slice(root.length);
    return [String(crate), ...underCrate.slice(0, d)].join('/');
  }

  // Dig OUT (d < 0): collapse the deepest crates into their parent folder first.
  // A crate at directory depth D keeps its full path while the cap (maxDepth + d)
  // is ≥ D, and is truncated to `cap` segments once the cap drops below D — so
  // the deepest branch merges at dig -1, the next at dig -2, … down to one root.
  const { dirOf, maxDepth } = crateDirs(level);
  const path = dirOf.get(String(crate)) || [];
  const cap  = maxDepth + d;
  const keep = path.slice(0, Math.max(0, cap));
  return keep.length ? keep.join('/') : '_root';
}

// How many group boxes the overview would show at a given dig level — the count
// of distinct group keys over the level's nodes. Used to preview the result of
// digging in/out under the dig control's +/- buttons. Returns null when the dig
// is out of range (so the caller can blank a disabled button's count).
function groupCountAtDig(level, dig) {
  const d = dig | 0;
  if (d > DIG_MAX || d < -maxCrateDepth(level)) return null;
  const keys = new Set();
  for (const n of (unionGraph(level).nodes || [])) keys.add(groupKeyAtDig(level, n, d));
  return keys.size;
}
window.groupCountAtDig = groupCountAtDig;

// A `groupOf(node)` closure for a given dig level. grouperForDig(level, 0)
// reproduces the legacy per-crate grouping.
function grouperForDig(level, dig) {
  return n => groupKeyAtDig(level, n, dig || 0);
}
window.grouperForDig = grouperForDig;

// Display label for a group node's box: the FULL folder path from the workspace
// root with a leading slash (e.g. `/crates/code-split-viewer/src/config`), never
// just `/src` or a leaf segment.
//  • dig 0 (crate tier): the crate value.
//  • dig IN (dig > 0): crate dir + absorbed source prefix + the folders under it.
//  • dig OUT (dig < 0 collapse): the collapsed crate-dir path (already full).
function groupLabel(level, key, dig) {
  const d = dig | 0;
  if (d > 0) {
    const cut     = key.indexOf('/');
    const crate   = cut >= 0 ? key.slice(0, cut) : key;
    const under   = cut >= 0 ? key.slice(cut + 1).split('/') : [];
    const root    = crateRoots(level).get(crate) || [];
    const crateD  = crateDirs(level).dirOf.get(crate) || [];
    const srcTail = root.slice(crateD.length);   // e.g. ['src'] absorbed into the crate root
    const full    = [...crateD, ...srcTail, ...under];   // FULL path from the workspace root
    return full.length ? '/' + full.join('/') : key;
  }
  if (d < 0) return key;   // dig OUT: full folder path, not the leaf segment
  return key.includes('/') ? key.slice(key.lastIndexOf('/') + 1) : key;
}
window.groupLabel = groupLabel;

// A node's directory RELATIVE TO ITS CRATE directory, with a leading slash
// (e.g. "/src/services"); "/" for a file sitting directly in the crate dir. Used
// for the drilled-view directory sub-cluster labels so they read `/src` rather
// than the full `crates/<crate>/src` path. Falls back to the full relativized dir
// when the node has no crate.
function crateRelDir(level, n) {
  const gk    = levelUi(level).grouping?.key;
  const segs  = relPathOf(n.id).split('/').slice(0, -1);   // dir segments
  const crate = gk ? n[gk] : null;
  if (crate == null || crate === '') return segs.length ? '/' + segs.join('/') : '/';
  const cdir = crateDirs(level).dirOf.get(String(crate)) || [];
  const rel  = segs.slice(cdir.length);
  return rel.length ? '/' + rel.join('/') : '/';
}
window.crateRelDir = crateRelDir;

// A node's full workspace-relative directory path with a leading slash
// (e.g. "/libs/modkit-odata-macros/src"); "/" for a file at the workspace root.
// Used for the drilled-view directory sub-cluster labels so they read in full
// rather than just the crate-relative tail.
function nodeFullDir(n) {
  const segs = relPathOf(n.id).split('/').slice(0, -1);   // drop the filename
  return segs.length ? '/' + segs.join('/') : '/';
}
window.nodeFullDir = nodeFullDir;

// Aggregate the per-node cycle statuses of a group's members into one status for
// the group node (used to red-stroke groups that contain a dependency cycle).
function aggCycleStatus(statuses) {
  let b = false, c = false, both = false;
  for (const s of statuses) {
    if (s === 'both') both = true;
    else if (s === 'baseline-only') b = true;
    else if (s === 'current-only') c = true;
  }
  if (both || (b && c)) return 'both';
  if (b) return 'baseline-only';
  if (c) return 'current-only';
  return 'none';
}
