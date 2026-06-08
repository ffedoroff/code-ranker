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
// Diameter for an aggregate (sum over all files in a group). Uses the same
// sqrt-scale formula but with a higher base so groups don't dwarf the canvas.
function metricGroupDiam(aggVal, mode) {
  if (mode === 'loc') return +(METRIC_BASE_DIAM * Math.sqrt(Math.max(aggVal, METRIC_BASE_LOC) / METRIC_BASE_LOC)).toFixed(3);
  if (mode === 'hk')  return aggVal === 0 ? 0.3 : +(METRIC_BASE_DIAM * Math.sqrt(Math.max(aggVal, METRIC_BASE_HK) / METRIC_BASE_HK)).toFixed(3);
  return 0.3;
}
function fmtMetricShort(v) {
  if (v >= 1_000_000) return Math.round(v / 1_000_000) + 'M';
  if (v >= 1_000)     return Math.round(v / 1_000) + 'K';
  return String(Math.round(v));
}
const metricFontSize = d => Math.max(6, Math.round(d * 26));

// Returns a `groupOf(node)` function for a given level, using the same grouping
// spec as buildDOT. Exported as a global so diagram.js can compute group stats.
function makeGroupOf(level) {
  const grouping = levelUi(level).grouping || {};
  const byKey    = grouping.key;
  const dirGrouper = n => {
    const p = n.id.replace(/^\{[^}]+\}\//, '');
    const i = p.lastIndexOf('/');
    return i > 0 ? p.slice(0, i) : '_root';
  };
  return n => {
    if (isExternalNode(n, level)) return (nodeKindSpec(level, n.kind).plural || 'external').toLowerCase();
    if (byKey) {
      const v = n[byKey];
      return (v === undefined || v === null || v === '') ? '(none)' : String(v);
    }
    return dirGrouper(n);
  };
}

function buildDOT(nodes, edges, level, viewport) {
  const sizeMode   = window.nodeSizeMode || null;
  const drillGroup = window.drillGroup   || null;
  const isMetric   = sizeMode === 'loc' || sizeMode === 'hk';
  const gOf        = makeGroupOf(level);

  let dot = 'digraph {\n';
  dot += '  rankdir=LR\n';
  if (viewport && viewport.w > 0 && viewport.h > 0) {
    const GV_IN = 72;
    const sw = (viewport.w / GV_IN).toFixed(4);
    const sh = (viewport.h / GV_IN).toFixed(4);
    dot += `  size="${sw},${sh}"\n`;
    dot += '  ratio=fill\n';
  }
  dot += '  graph [bgcolor="white" fontname="Helvetica" pad="0.5" nodesep="0.25" ranksep="1.0"]\n';
  if (isMetric) {
    dot += '  node  [shape=circle style=filled fixedsize=true width=0.3]\n\n';
  } else {
    dot += '  node  [shape=box style=filled fontname="Helvetica" fontsize=11]\n\n';
  }

  // ── Group view: one node per group, deduped inter-group edges ─────────────────
  if (drillGroup === null) {
    const nodeGroup  = new Map();
    const groupNodes = new Map();
    for (const n of nodes) {
      const g = gOf(n);
      nodeGroup.set(n.id, g);
      if (!groupNodes.has(g)) groupNodes.set(g, []);
      groupNodes.get(g).push(n);
    }

    const baselineById = new Map((window.BASELINE?.graphs?.[level]?.nodes || []).map(n => [n.id, n]));
    const currentById  = new Map((window.CURRENT?.graphs?.[level]?.nodes  || []).map(n => [n.id, n]));

    for (const [g, gNodes] of groupNodes) {
      if (isMetric) {
        const aggB = gNodes.reduce((s, n) => s + metricNodeVal(baselineById.get(n.id), sizeMode), 0);
        const aggC = gNodes.reduce((s, n) => s + metricNodeVal(currentById.get(n.id),  sizeMode), 0);
        const agg  = Math.max(aggB, aggC) || gNodes.reduce((s, n) => s + metricNodeVal(n, sizeMode), 0);
        const d    = metricGroupDiam(agg, sizeMode);
        const lbl  = agg > 0 ? fmtMetricShort(agg) : '';
        const fs   = metricFontSize(d);
        dot += `  ${dotId(g)} [label=${dotId(lbl)} fontsize=${fs} fontcolor="#333" fillcolor="#ffd4d4" color="${N_COLOR}" width=${d} shape=circle style=filled fixedsize=true]\n`;
      } else {
        dot += `  ${dotId(g)} [label=${dotId(g)} fillcolor="#ffd4d4" color="${N_COLOR}" shape=box style=filled fontname="Helvetica" fontsize=11]\n`;
      }
    }

    const seenGroupEdge = new Set();
    for (const e of edges) {
      if (!edgeIsFlow(level, e.kind)) continue;
      const sg = nodeGroup.get(e.source);
      const tg = nodeGroup.get(e.target);
      if (!sg || !tg || sg === tg) continue;
      const key = sg + '\x00' + tg;
      if (seenGroupEdge.has(key)) continue;
      seenGroupEdge.add(key);
      dot += `  ${dotId(sg)} -> ${dotId(tg)} [color="${E_COLOR}" style="solid"]\n`;
    }

    dot += '}';
    return dot;
  }

  // ── Drilled file view: only files in the selected group ───────────────────────
  const drillNodes = nodes.filter(n => gOf(n) === drillGroup);
  const drillIds   = new Set(drillNodes.map(n => n.id));
  dot += '  newrank=true\n';

  const baselineById = new Map((window.BASELINE?.graphs?.[level]?.nodes || []).map(n => [n.id, n]));
  const currentById  = new Map((window.CURRENT?.graphs?.[level]?.nodes  || []).map(n => [n.id, n]));
  const allNodesById = new Map(nodes.map(n => [n.id, n]));

  const layoutDiam = n => {
    const db = baselineById.has(n.id) ? metricNodeDiam(baselineById.get(n.id), sizeMode) : 0;
    const da = currentById.has(n.id)  ? metricNodeDiam(currentById.get(n.id),  sizeMode) : 0;
    return Math.max(db, da) || metricNodeDiam(n, sizeMode);
  };

  const eAttr = e =>
    `color="${E_COLOR}" style="solid" class="edge-${e.kind || 'unknown'} status-${e.status} cycle-status-none"`;

  const nAttr = n => {
    const ks   = nodeKindSpec(level, n.kind);
    const ext  = isExternalNode(n, level);
    const fill = ks.fill   || (ext ? EXT_FILL  : N_FILL);
    const col  = ks.stroke || (ext ? EXT_COLOR : N_COLOR);
    const cls  = `class="node-${n.kind || 'unknown'} status-${n.status} cycle-status-none"`;
    if (isMetric) {
      const d   = layoutDiam(n);
      const v   = metricNodeVal(n, sizeMode);
      const lbl = v > 0 ? fmtMetricShort(v) : '';
      const fs  = metricFontSize(d);
      return `label=${dotId(lbl)} fontsize=${fs} fontcolor="#333" fillcolor="${fill}" color="${col}" width=${d} ${cls}`;
    }
    return `label=${dotId(n.name)} fillcolor="${fill}" color="${col}" ${cls}`;
  };

  // ── Collect external neighbor groups (no 3rd-party) ───────────────────────────
  // inGrpFiles: groups that call INTO our files (left side)
  // outGrpFiles: groups that our files call OUT TO (right side)
  // A group in both → only appears on the left.
  const inGrpFiles  = new Map(); // group → Set<our-file-id>
  const outGrpFiles = new Map(); // group → Set<our-file-id>
  for (const e of edges) {
    const sIn = drillIds.has(e.source), tIn = drillIds.has(e.target);
    if (!sIn && tIn) {
      const src = allNodesById.get(e.source);
      if (!src || isExternalNode(src, level)) continue;
      const g = gOf(src);
      if (g === drillGroup) continue;
      if (!inGrpFiles.has(g)) inGrpFiles.set(g, new Set());
      inGrpFiles.get(g).add(e.target);
    } else if (sIn && !tIn) {
      const tgt = allNodesById.get(e.target);
      if (!tgt || isExternalNode(tgt, level)) continue;
      const g = gOf(tgt);
      if (g === drillGroup) continue;
      if (!outGrpFiles.has(g)) outGrpFiles.set(g, new Set());
      outGrpFiles.get(g).add(e.source);
    }
  }
  // Groups in both → remove from outGrpFiles (they appear left only)
  for (const g of inGrpFiles.keys()) outGrpFiles.delete(g);

  const IN_EDGE_COLOR  = '#88bb88';
  const OUT_EDGE_COLOR = '#ccaa77';
  const IN_FILL        = '#edf7ed';
  const OUT_FILL       = '#fdf3e3';

  // Node style for external group boxes in the neighbor clusters
  // Always boxes regardless of metric mode — fixedsize/width from global node default must be reset.
  const extNode = (g, borderColor, fillColor) =>
    `[label=${dotId(g)} fillcolor="${fillColor}" color="${borderColor}" shape=box style=filled fixedsize=false fontname="Helvetica" fontsize=11]`;
  const inNodeId  = g => 'IN\x01' + g;
  const outNodeId = g => 'OUT\x01' + g;

  // Left cluster — callers of this group
  if (inGrpFiles.size > 0) {
    dot += `  subgraph cluster_in {\n`;
    dot += `    label="callers" style=filled fillcolor="${IN_FILL}" color="#88bb88" fontcolor="#447744" fontname="Helvetica" fontsize=10\n`;
    for (const g of inGrpFiles.keys())
      dot += `    ${dotId(inNodeId(g))} ${extNode(g, IN_EDGE_COLOR, IN_FILL)}\n`;
    dot += '  }\n';
  }

  // Sub-clusters by directory within the drilled group
  const dirOf = n => {
    const p = n.id.replace(/^\{[^}]+\}\//, '');
    const i = p.lastIndexOf('/');
    return i > 0 ? p.slice(0, i) : '_root';
  };
  const subGroups = new Map();
  drillNodes.forEach(n => { const d = dirOf(n); (subGroups.get(d) || subGroups.set(d, []).get(d)).push(n); });
  let si = 0;
  for (const [label, ns] of subGroups) {
    dot += `  subgraph cluster_${si++} {\n`;
    dot += `    label=${dotId(label)} color="#cccccc" fontcolor="#666666"\n`;
    for (const n of ns) dot += `    ${dotId(n.id)} [${nAttr(n)}]\n`;
    dot += '  }\n';
  }

  // Right cluster — dependencies of this group
  if (outGrpFiles.size > 0) {
    dot += `  subgraph cluster_out {\n`;
    dot += `    label="dependencies" style=filled fillcolor="${OUT_FILL}" color="#ccaa77" fontcolor="#886633" fontname="Helvetica" fontsize=10\n`;
    for (const g of outGrpFiles.keys())
      dot += `    ${dotId(outNodeId(g))} ${extNode(g, OUT_EDGE_COLOR, OUT_FILL)}\n`;
    dot += '  }\n';
  }

  // Pin callers strictly left, dependencies strictly right
  if (inGrpFiles.size > 0) {
    dot += '  { rank=min';
    for (const g of inGrpFiles.keys()) dot += `; ${dotId(inNodeId(g))}`;
    dot += ' }\n';
  }
  if (outGrpFiles.size > 0) {
    dot += '  { rank=max';
    for (const g of outGrpFiles.keys()) dot += `; ${dotId(outNodeId(g))}`;
    dot += ' }\n';
  }

  // ── Edges ─────────────────────────────────────────────────────────────────────
  // Internal edges (within the drilled group)
  const seenEdge = new Set();
  for (const e of edges) {
    if (!drillIds.has(e.source) || !drillIds.has(e.target)) continue;
    const key = e.source + '\x00' + e.target;
    if (seenEdge.has(key)) continue;
    seenEdge.add(key);
    dot += `  ${dotId(e.source)} -> ${dotId(e.target)} [${eAttr(e)}]\n`;
  }

  // Inbound group → our file (one edge per inGroup+file pair)
  for (const [g, files] of inGrpFiles) {
    const src = dotId(inNodeId(g));
    for (const fid of files)
      dot += `  ${src} -> ${dotId(fid)} [color="${IN_EDGE_COLOR}" style="solid" class="edge-in"]\n`;
    // If this group is also an outbound group (both roles), draw those edges too
    if (outGrpFiles.has(g)) {
      for (const fid of outGrpFiles.get(g))
        dot += `  ${dotId(fid)} -> ${src} [color="${IN_EDGE_COLOR}" style="solid" class="edge-in"]\n`;
    }
  }
  // Our file → outbound group
  for (const [g, files] of outGrpFiles) {
    const tgt = dotId(outNodeId(g));
    for (const fid of files)
      dot += `  ${dotId(fid)} -> ${tgt} [color="${OUT_EDGE_COLOR}" style="solid" class="edge-out"]\n`;
  }

  dot += '}';
  return dot;
}
