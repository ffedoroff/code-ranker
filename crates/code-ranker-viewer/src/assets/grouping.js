// grouping.js — the grouping ladder for the map's relative "dig" (level-of-detail).
//
// Two orthogonal navigation axes (see docs/code-ranker-viewer/REFACTOR-split-plan.md):
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

const DIG_MIN = -12, DIG_MAX = 8;
function clampDig(z) { return Math.max(DIG_MIN, Math.min(DIG_MAX, (z | 0))); }
window.clampDig = clampDig;

// The grouping DIMENSION the map slices by — the breadcrumb's tier dropdown:
//   • 'crate' — group by the crate attribute (Rust modules), the default when the
//               level declares a grouping key;
//   • 'file'  — ignore the crate attribute and group purely by directory.
// `window.tier` (set by the dropdown) overrides; otherwise fall back to 'crate'
// when a grouping key exists, else 'file'.
function viewTier(level) {
  if (window.tier === 'crate' || window.tier === 'file') return window.tier;
  return levelUi(level).grouping?.key ? 'crate' : 'file';
}
window.viewTier = viewTier;

// A node's crate id (the `grouping.key` attribute) as a non-empty string, or null.
function crateIdOf(level, n) {
  const gk = levelUi(level).grouping?.key;
  const c  = gk ? n[gk] : null;
  return (c != null && c !== '') ? String(c) : null;
}
window.crateIdOf = crateIdOf;

// A node id's directory segments (path minus the `{token}` root marker and the
// filename). The unit most path math here starts from.
function nodeDirSegs(id) { return relPathOf(id).split('/').slice(0, -1); }
window.nodeDirSegs = nodeDirSegs;

// The default-focus directory: descend from the root through any chain of folders
// that hold exactly ONE subfolder and NO direct files, stopping at the first
// folder that branches (>1 subfolder) or holds a file. Returns the dir segments
// ([] = the root itself already branches → no auto-drill).
function autoFocusSegs(level) {
  const dirs = (unionGraph(level).nodes || [])
    .filter(n => !isExternalNode(n, level))
    .map(n => nodeDirSegs(n.id));
  const prefix = [];
  for (;;) {
    let hasDirectFile = false;
    const subdirs = new Set();
    for (const segs of dirs) {
      if (segs.length < prefix.length) continue;
      let under = true;
      for (let i = 0; i < prefix.length; i++) if (segs[i] !== prefix[i]) { under = false; break; }
      if (!under) continue;
      if (segs.length === prefix.length) hasDirectFile = true;
      else subdirs.add(segs[prefix.length]);
    }
    if (!hasDirectFile && subdirs.size === 1) { prefix.push([...subdirs][0]); continue; }
    break;
  }
  return prefix;
}
window.autoFocusSegs = autoFocusSegs;

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
      const dirs = nodeDirSegs(n.id);   // drop the filename
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

// The deepest directory nesting over all nodes — the file-tier counterpart of
// maxCrateDepth (how far dig-OUT can collapse the pure-directory grouping before
// reaching a single root). Memoised per level.
const _fileDepthCache = new Map();
function maxFileDepth(level) {
  if (_fileDepthCache.has(level)) return _fileDepthCache.get(level);
  let m = 0;
  for (const n of (unionGraph(level).nodes || [])) {
    if (isExternalNode(n, level)) continue;
    const d = relPathOf(n.id).split('/').length - 1;   // dir segments
    if (d > m) m = d;
  }
  _fileDepthCache.set(level, m);
  return m;
}
window.maxFileDepth = maxFileDepth;

// The deepest folder nesting UNDER a crate root (crate tier) — the dig at which the
// per-crate folder grouping is fully expanded (every directory segment revealed).
// Digging one step past it switches the overview to individual files. Memoised.
const _underDepthCache = new Map();
function maxUnderCrateDepth(level) {
  if (_underDepthCache.has(level)) return _underDepthCache.get(level);
  const gk    = levelUi(level).grouping?.key;
  const roots = crateRoots(level);
  let m = 0;
  if (gk) {
    for (const n of (unionGraph(level).nodes || [])) {
      if (isExternalNode(n, level)) continue;
      const crate = n[gk];
      if (crate == null || crate === '') continue;
      const root = roots.get(String(crate)) || [];
      const d    = nodeDirSegs(n.id).length - root.length;
      if (d > m) m = d;
    }
  }
  _underDepthCache.set(level, m);
  return m;
}
window.maxUnderCrateDepth = maxUnderCrateDepth;

// The reveal dig at which the overview shows individual FILES — one step past the
// deepest folder grouping (crate tier: under-crate depth; file tier: the finest
// per-directory grouping at dig 0). At or beyond it every node is its own group.
function filesDig(level) {
  return (viewTier(level) === 'file' ? 0 : maxUnderCrateDepth(level)) + 1;
}
window.filesDig = filesDig;
function isFilesDig(level, dig) { return (dig | 0) >= filesDig(level); }
window.isFilesDig = isFilesDig;

// The dig-OUT floor for the active tier: how far the grouping can collapse before
// it is a single root group. Crate tier collapses crate-dir paths; file tier
// collapses plain directory paths.
function digFloor(level) {
  return -(viewTier(level) === 'file' ? maxFileDepth(level) : maxCrateDepth(level));
}
window.digFloor = digFloor;

// The overview's default landing dig — where reveal depth reads 0. Crate tier
// lands on the crates (dig 0); file tier lands one level below the root (top
// directories) rather than the finest per-folder grouping dig 0 would give.
function overviewBaseDig(level) {
  return viewTier(level) === 'file' ? clampDig(digFloor(level) + 1) : 0;
}
window.overviewBaseDig = overviewBaseDig;

// Snapshot swaps change the node set → drop the memoised caches.
function clearGroupingCache() { _crateRootCache.clear(); _crateDirsCache.clear(); _fileDepthCache.clear(); _underDepthCache.clear(); }
window.clearGroupingCache = clearGroupingCache;

// Group key for a node at a given dig level. dig 0 → the crate value (matches the
// legacy grouping); dig>0 appends directory segments under the crate; dig<0
// collapses the deepest crates into their ancestor folders, deepest first.
function groupKeyAtDig(level, n, dig) {
  if (isExternalNode(n, level))
    return (nodeKindSpec(level, n.kind).plural || 'external').toLowerCase();

  const d     = dig | 0;
  // One reveal step past the deepest folder grouping: every node is its own group
  // (keyed by node id, like the drilled file view) so edges read file↔file; the
  // overview renderer still wraps them in their deepest-folder cluster.
  if (isFilesDig(level, d)) return n.id;
  const gk    = levelUi(level).grouping?.key;
  // File tier ignores the crate attribute → plain directory tiers for every node.
  const crate = (viewTier(level) === 'crate' && gk) ? n[gk] : null;
  const dirs  = nodeDirSegs(n.id);

  // No crate attribute (file tier, or a crate-less level): plain directory tiers
  // on an ABSOLUTE ladder — dig 0 keeps the file's full directory (finest folder
  // grouping), each step out drops one global level until dig = -maxFileDepth is a
  // single root. Absolute (not per-node-relative) so a fixed directory key has a
  // single well-defined dig — which makes file-tier drilling unambiguous.
  if (crate == null || crate === '') {
    const keepN = Math.max(0, Math.min(dirs.length, maxFileDepth(level) + d));
    const keep  = dirs.slice(0, keepN);
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
  if (d > DIG_MAX || d < digFloor(level)) return null;
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
// root with a leading slash (e.g. `/crates/code-ranker-viewer/src/config`), never
// just `/src` or a leaf segment.
//  • dig 0 (crate tier): the crate value.
//  • dig IN (dig > 0): crate dir + absorbed source prefix + the folders under it.
//  • dig OUT (dig < 0 collapse): the collapsed crate-dir path (already full).
function groupLabel(level, key, dig) {
  const d = dig | 0;
  if (key === '_root') return '/';   // the collapse sentinel → show the root as "/"
  // Files mode: the key is the file's node id → show just the file name.
  if (isFilesDig(level, d)) { const p = relPathOf(key); return p.includes('/') ? p.slice(p.lastIndexOf('/') + 1) : p; }
  // File tier: the key IS the full workspace-relative directory path; show it with
  // a leading slash (no crate-segment logic).
  if (viewTier(level) === 'file') return '/' + key;
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

// The current focus's own directory as a workspace path with a leading slash (the
// breadcrumb path) — the longest common directory of the focused group's files;
// '' at the overview. Used to render drilled folder labels RELATIVE to the focus.
function focusDirPath(level) {
  const grp = window.drillGroup;
  if (grp == null) return '';
  const gOf = grouperForDig(level, window.drillDig ?? 0);
  let common = null;
  for (const n of (unionGraph(level).nodes || [])) {
    if (gOf(n) !== grp) continue;
    const segs = nodeDirSegs(n.id);
    if (common === null) common = segs.slice();
    else { let i = 0; while (i < common.length && i < segs.length && common[i] === segs[i]) i++; common.length = i; }
  }
  return (common && common.length) ? '/' + common.join('/') : '';
}
window.focusDirPath = focusDirPath;

// The prefix subtracted from drilled folder labels: the focus's PARENT directory,
// so the focus folder itself keeps its name (focus `…/sdk/src` → its cluster reads
// `/src`, children `/src/render`) while the long ancestor path is dropped.
function focusStripBase(level) {
  const f = focusDirPath(level);
  if (!f || f === '/') return '';
  const i = f.lastIndexOf('/');
  return i > 0 ? f.slice(0, i) : '';
}
window.focusStripBase = focusStripBase;

// Strip a focus dir prefix `base` from an absolute folder path, leaving a
// focus-relative path with a leading slash; "/" when the path IS the focus dir.
function stripDirPrefix(base, full) {
  if (base && (full === base || full.startsWith(base + '/'))) return full.slice(base.length) || '/';
  return full;
}
window.stripDirPrefix = stripDirPrefix;

// A node's directory RELATIVE TO ITS CRATE directory, with a leading slash
// (e.g. "/src/services"); "/" for a file sitting directly in the crate dir. Used
// for the drilled-view directory sub-cluster labels so they read `/src` rather
// than the full `crates/<crate>/src` path. Falls back to the full relativized dir
// when the node has no crate.
function crateRelDir(level, n) {
  const gk    = levelUi(level).grouping?.key;
  const segs  = nodeDirSegs(n.id);   // dir segments
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
  const segs = nodeDirSegs(n.id);   // drop the filename
  return segs.length ? '/' + segs.join('/') : '/';
}
window.nodeFullDir = nodeFullDir;

// ── Tier switching: map a focus key across the crate ⇄ file dimensions ──────────
// A crate ≡ its source directory, so the two key spaces meet at the crate-root
// boundary: above it the keys are directory paths, below it they are identical
// folder tails. These translate a focus group key from one tier to the other,
// returning null when no mapping exists (the caller falls back to the anchor).

// crate-tier key (`crate` or `crate/folder/…`) → file-tier key (a directory path).
// Expands the leading crate segment into the crate's real directory; keeps the
// folder tail (already crate-relative, so it appends directly).
function crateKeyToFileKey(level, key) {
  if (key == null || key === '_root') return '_root';
  const cut   = key.indexOf('/');
  const crate = cut >= 0 ? key.slice(0, cut) : key;
  const tail  = cut >= 0 ? key.slice(cut + 1).split('/') : [];
  const root  = crateRoots(level).get(String(crate));
  if (!root) return null;                         // crate not found
  const full  = [...root, ...tail];
  return full.length ? full.join('/') : '_root';
}
window.crateKeyToFileKey = crateKeyToFileKey;

// file-tier key (a directory path) → crate-tier key. Finds the crate whose root
// directory is the deepest prefix of the path and collapses that prefix into the
// crate segment, keeping the folder tail. Returns null for a path inside no crate
// (the caller then falls back to the nearest representable ancestor / anchor).
function fileKeyToCrateKey(level, key) {
  if (key == null || key === '_root') return null;   // overview anchor
  const segs = key.split('/');
  let best = null, bestLen = -1;
  for (const [crate, root] of crateRoots(level)) {
    if (root.length > segs.length) continue;
    let ok = true;
    for (let i = 0; i < root.length; i++) if (root[i] !== segs[i]) { ok = false; break; }
    if (ok && root.length > bestLen) { best = String(crate); bestLen = root.length; }
  }
  if (best == null) return null;                  // not inside any crate
  const tail = segs.slice(bestLen);
  return [best, ...tail].join('/');
}
window.fileKeyToCrateKey = fileKeyToCrateKey;

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
