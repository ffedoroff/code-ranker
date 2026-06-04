// Fallback palette (used only when the snapshot's node_kinds dictionary omits a
// colour). Real colours come from node_kinds[kind].fill / .stroke.
const N_FILL  = '#dbe9f4';
const N_COLOR = '#4d6f9c';
const E_COLOR = '#4d6f9c';
const EXT_FILL  = '#f6e2c0';
const EXT_COLOR = '#b3801f';

function dotId(id) {
  return '"' + id.replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"';
}

// ── Metric node sizing (loc/hk circle modes) — reads flat node attributes.
// Module-scope so the post-layout per-side resize (`applySideSizing`) reuses the
// exact same math. The size-mode key maps to an attribute: 'loc' → sloc (the
// source-line count, falling back to the structural loc), 'hk' → hk. ──
const METRIC_BASE_DIAM = 0.3, METRIC_BASE_LOC = 100, METRIC_BASE_HK = 1000;
function metricNodeVal(n, mode) {
  if (!n) return 0;
  if (mode === 'loc') return Number(n.sloc ?? n.loc ?? 0);
  if (mode === 'hk')  return Number(n.hk ?? 0);
  return 0;
}
function metricNodeDiam(n, mode) {
  const v = metricNodeVal(n, mode);
  if (mode === 'loc') return +(METRIC_BASE_DIAM * Math.sqrt(Math.max(v, METRIC_BASE_LOC) / METRIC_BASE_LOC)).toFixed(3);
  if (mode === 'hk')  return v === 0 ? 0.3 : +(METRIC_BASE_DIAM * Math.sqrt(Math.max(v, METRIC_BASE_HK) / METRIC_BASE_HK)).toFixed(3);
  return 0.3;
}
function fmtMetricShort(v) {
  if (v >= 1_000_000) return Math.round(v / 1_000_000) + 'M';
  if (v >= 1_000)     return Math.round(v / 1_000) + 'K';
  return String(Math.round(v));
}
const metricFontSize = d => Math.max(6, Math.round(d * 26));

function buildDOT(nodes, edges, level) {
  const cycles = window.CYCLES?.[level];
  const mode = window.nodeSizeMode || 'default';

  const beforeById = new Map((window.BEFORE?.graphs?.[level]?.nodes || []).map(n => [n.id, n]));
  const afterById  = new Map((window.AFTER?.graphs?.[level]?.nodes  || []).map(n => [n.id, n]));
  const layoutDiam = n => {
    const db = beforeById.has(n.id) ? metricNodeDiam(beforeById.get(n.id), mode) : 0;
    const da = afterById.has(n.id)  ? metricNodeDiam(afterById.get(n.id),  mode) : 0;
    return Math.max(db, da) || metricNodeDiam(n, mode);
  };

  const nodeVal  = n => metricNodeVal(n, mode);
  const fmtShort = fmtMetricShort;

  const nAttr = n => {
    const cs   = cycles?.nodeCycleStatus?.get(n.id) || 'none';
    const ks   = nodeKindSpec(level, n.kind);
    const ext  = isExternalNode(n, level);
    const fill = ks.fill   || (ext ? EXT_FILL  : N_FILL);
    const col  = ks.stroke || (ext ? EXT_COLOR : N_COLOR);
    const cls  = `class="node-${n.kind || 'unknown'} status-${n.status} cycle-status-${cs}"`;
    if (mode === 'default') {
      return `label=${dotId(n.name)} fillcolor="${fill}" color="${col}" ${cls}`;
    }
    const d   = layoutDiam(n);
    const v   = nodeVal(n);
    const lbl = v > 0 ? fmtShort(v) : '';
    const fs  = metricFontSize(d);
    return `label=${dotId(lbl)} fontsize=${fs} fontcolor="#333" fillcolor="${fill}" color="${col}" width=${d} ${cls}`;
  };
  const eAttr = e => {
    const cs  = cycles?.edgeCycleStatus?.(e.source, e.target) || 'none';
    return `color="${E_COLOR}" style="solid" class="edge-${e.kind || 'unknown'} status-${e.status} cycle-status-${cs}"`;
  };

  let dot = 'digraph {\n';
  dot += '  rankdir=LR\n';
  dot += '  graph [bgcolor="white" fontname="Helvetica" pad="0.5" nodesep="0.25" ranksep="1.0"]\n';
  if (mode === 'default') {
    dot += '  node  [shape=box style=filled fontname="Helvetica" fontsize=11]\n\n';
  } else {
    dot += '  node  [shape=circle style=filled fixedsize=true width=0.3]\n\n';
  }

  // Cluster nodes per the level's `ui.grouping` spec:
  //   { key: "crate" }      → group by the value of that node attribute
  //   { function: "dir" }   → a named grouper implemented here
  //   (absent)              → default `dir` grouper (folder from the path)
  // The group's value doubles as the cluster label. External nodes always go to
  // their own kind bucket (they have no source attributes like `crate`).
  const GROUPERS = {
    // Full project-relative directory path (the `{root}/` token stripped), so
    // nested folders like `crates/code-split-cli/src/config` stay unambiguous.
    dir: n => {
      const p = n.id.replace(/^\{[^}]+\}\//, '');
      const i = p.lastIndexOf('/');
      return i > 0 ? p.slice(0, i) : '_root';
    },
  };
  const grouping = levelUi(level).grouping || {};
  const byKey = grouping.key;
  const grouper = GROUPERS[grouping.function] || GROUPERS.dir;
  const groupOf = n => {
    if (isExternalNode(n, level)) return (nodeKindSpec(level, n.kind).plural || 'external').toLowerCase();
    if (byKey) {
      const v = n[byKey];
      return (v === undefined || v === null || v === '') ? '(none)' : String(v);
    }
    return grouper(n);
  };
  const groups = new Map();
  nodes.forEach(n => { const g = groupOf(n); (groups.get(g) || groups.set(g, []).get(g)).push(n); });
  let i = 0;
  for (const [label, ns] of groups) {
    dot += `  subgraph cluster_${i++} {\n`;
    dot += `    label=${dotId(label)} color="#cccccc" fontcolor="#666666"\n`;
    for (const n of ns) dot += `    ${dotId(n.id)} [${nAttr(n)}]\n`;
    dot += '  }\n';
  }

  // At most one drawn edge per (source, target) pair; structural (non-flow) edge
  // kinds are kept in the data but not drawn (read from edge_kinds[kind].flow).
  const seenEdge = new Set();
  for (const e of edges) {
    if (!edgeIsFlow(level, e.kind)) continue;
    const key = e.source + ' ' + e.target;
    if (seenEdge.has(key)) continue;
    seenEdge.add(key);
    dot += `  ${dotId(e.source)} -> ${dotId(e.target)} [${eAttr(e)}]\n`;
  }

  dot += '}';
  return dot;
}
