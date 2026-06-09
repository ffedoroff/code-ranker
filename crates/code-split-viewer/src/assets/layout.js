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

// The grouping ladder (`grouperForDig`) lives in grouping.js; layout consumes it.

function buildDOT(nodes, edges, level, viewport) {
  const sizeMode   = window.nodeSizeMode || null;
  const drillGroup = window.drillGroup   || null;
  const isMetric   = sizeMode === 'loc' || sizeMode === 'hk';
  // Overview granularity follows the relative zoom; a drilled (focus) view filters
  // by the zoom that was active when the user drilled in.
  const activeDig  = drillGroup === null ? (window.dig || 0) : (window.drillDig ?? 0);
  const gOf        = grouperForDig(level, activeDig);
  const cycleOf    = window.CYCLES?.[level]?.nodeCycleStatus;
  // Cycle filter: when on, keep only nodes that sit in a dependency cycle (and
  // the edges between them); callers/dependencies clusters are kept as usual.
  const cycleOnly  = !!window.cycleOnly;
  const isCyc      = id => !!(cycleOf && cycleOf.has(id));

  let dot = 'digraph {\n';
  dot += '  rankdir=LR\n';
  // No `ratio=fill` / `size`: let graphviz lay out at natural size with packed
  // nodes (tiny nodesep/ranksep), then the SVG viewBox scales uniformly to the
  // frame — so the gaps between nodes stay small instead of being stretched.
  // Tighter rank/node spacing + roomier box padding so nodes occupy more of the
  // frame relative to whitespace (edges route less prettily — an accepted trade
  // for bigger, more legible nodes).
  dot += '  graph [bgcolor="white" fontname="Helvetica" pad="0.1" nodesep="0.12" ranksep="0.6"]\n';
  // Smaller arrowheads — graphviz default (arrowsize=1) reads oversized once the
  // SVG viewBox is scaled up to fill the frame on sparse graphs.
  dot += '  edge  [arrowsize=0.6]\n';
  if (isMetric) {
    dot += '  node  [shape=circle style=filled fixedsize=true width=0.3]\n\n';
  } else {
    dot += '  node  [shape=box style=filled fontname="Helvetica" fontsize=11 margin="0.044,0.022" height=0 width=0]\n\n';
  }

  // ── Group view: one node per group, deduped inter-group edges ─────────────────
  if (drillGroup === null) {
    const nodeGroup  = new Map();
    const groupNodes = new Map();
    for (const n of nodes) {
      if (cycleOnly && !isCyc(n.id)) continue;   // cycle filter: drop non-cycle nodes
      const g = gOf(n);
      nodeGroup.set(n.id, g);
      if (!groupNodes.has(g)) groupNodes.set(g, []);
      groupNodes.get(g).push(n);
    }

    const baselineById = new Map((window.BASELINE?.graphs?.[level]?.nodes || []).map(n => [n.id, n]));
    const currentById  = new Map((window.CURRENT?.graphs?.[level]?.nodes  || []).map(n => [n.id, n]));

    // Crate-tier groups (zoom 0) are pink; any other grouping (by folder) is a
    // uniform neutral white, so the colour signals "these are crates".
    const isCrateTier = activeDig === 0 && !!(levelUi(level).grouping?.key);
    const groupFill   = isCrateTier ? '#ffd4d4' : '#ffffff';
    // Metric circles are always filled — red for the crate tier, blue otherwise
    // (white reads as "empty" / unfinished on the folder tiers).
    const circleFill  = isCrateTier ? '#ffd4d4' : N_FILL;

    // One DOT statement for a single group box (circle in metric mode, box otherwise).
    const groupBoxDot = (g, gNodes) => {
      // A group is red when any member sits in a dependency cycle (aggregated
      // per side); reuses the same cycle-status CSS as individual nodes.
      const gCyc = aggCycleStatus(gNodes.map(n => cycleOf?.get(n.id) || 'none'));
      const cyc  = `class="cycle-status-${gCyc}"`;
      // Group label: crate name at dig 0, the full folder path when digging in
      // or collapsing (see grouping.js).
      const leaf = groupLabel(level, g, activeDig);
      if (isMetric) {
        const aggB = gNodes.reduce((s, n) => s + metricNodeVal(baselineById.get(n.id), sizeMode), 0);
        const aggC = gNodes.reduce((s, n) => s + metricNodeVal(currentById.get(n.id),  sizeMode), 0);
        const agg  = Math.max(aggB, aggC) || gNodes.reduce((s, n) => s + metricNodeVal(n, sizeMode), 0);
        const d    = metricGroupDiam(agg, sizeMode);
        const lbl  = agg > 0 ? fmtMetricShort(agg) : '';
        const fs   = metricFontSize(d);
        return `${dotId(g)} [label=${dotId(lbl)} fontsize=${fs} fontcolor="#333" fillcolor="${circleFill}" color="${N_COLOR}" width=${d} shape=circle style=filled fixedsize=true ${cyc}]`;
      }
      // Group box: name + the count of member nodes (what opens on drill-in).
      const lbl = `${leaf} (${gNodes.length})`;
      return `${dotId(g)} [label=${dotId(lbl)} fillcolor="${groupFill}" color="${N_COLOR}" shape=box style=filled fontname="Helvetica" fontsize=11 ${cyc}]`;
    };

    // At dig IN (>0) with crate grouping, wrap each crate's folder-groups in a
    // labelled crate cluster — so folders read as "inside their crate", mirroring
    // the drilled view's directory sub-clusters. dig 0 / dig OUT render flat.
    const clusterByCrate = activeDig > 0 && !!(levelUi(level).grouping?.key);
    if (clusterByCrate) {
      const crateOf = g => { const i = g.indexOf('/'); return i >= 0 ? g.slice(0, i) : g; };
      const byCrate = new Map();   // crate → [[g, gNodes], …]
      const loose   = [];          // external / crate-less groups stay outside clusters
      for (const [g, gNodes] of groupNodes) {
        if (gNodes.every(n => isExternalNode(n, level))) { loose.push([g, gNodes]); continue; }
        const c = crateOf(g);
        (byCrate.get(c) || byCrate.set(c, []).get(c)).push([g, gNodes]);
      }
      let ci = 0;
      for (const [crate, entries] of byCrate) {
        dot += `  subgraph cluster_crate_${ci++} {\n`;
        dot += `    label=${dotId(crate)} style=filled fillcolor="#fff2f2" color="#e3b3b3" fontname="Helvetica" fontsize=11 fontcolor="#a05a5a"\n`;
        for (const [g, gNodes] of entries) dot += `    ${groupBoxDot(g, gNodes)}\n`;
        dot += '  }\n';
      }
      for (const [g, gNodes] of loose) dot += `  ${groupBoxDot(g, gNodes)}\n`;
    } else {
      for (const [g, gNodes] of groupNodes) dot += `  ${groupBoxDot(g, gNodes)}\n`;
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
  const drillNodes = nodes.filter(n => gOf(n) === drillGroup && (!cycleOnly || isCyc(n.id)));
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

  const edgeCycleOf = window.CYCLES?.[level]?.edgeCycleStatus;
  const eAttr = e =>
    `color="${E_COLOR}" style="solid" class="edge-${e.kind || 'unknown'} status-${e.status} cycle-status-${edgeCycleOf ? edgeCycleOf(e.source, e.target) : 'none'}"`;

  const nAttr = n => {
    const ks   = nodeKindSpec(level, n.kind);
    const ext  = isExternalNode(n, level);
    const fill = ks.fill   || (ext ? EXT_FILL  : N_FILL);
    const col  = ks.stroke || (ext ? EXT_COLOR : N_COLOR);
    const cls  = `class="node-${n.kind || 'unknown'} status-${n.status} cycle-status-${cycleOf?.get(n.id) || 'none'}"`;
    if (isMetric) {
      const d   = layoutDiam(n);
      const v   = metricNodeVal(n, sizeMode);
      const lbl = v > 0 ? fmtMetricShort(v) : '';
      const fs  = metricFontSize(d);
      return `label=${dotId(lbl)} fontsize=${fs} fontcolor="#333" fillcolor="${fill}" color="${col}" width=${d} ${cls}`;
    }
    // File box: just the file name, no connection counts.
    return `label=${dotId(n.name)} fillcolor="${fill}" color="${col}" ${cls}`;
  };

  // ── Collect external neighbor groups (no 3rd-party) ───────────────────────────
  // inGrpFiles: groups that call INTO our files (left side)
  // outGrpFiles: groups that our files call OUT TO (right side)
  // A group in both → only appears on the left.
  const inGrpFiles  = new Map(); // group → Set<our-file-id>
  const outGrpFiles = new Map(); // group → Set<our-file-id>
  for (const e of edges) {
    if (!edgeIsFlow(level, e.kind)) continue;   // map shows only flow connections
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

  // Neighbour (callers/dependencies) labels: when every neighbour lives in the
  // SAME crate as the drilled group, drop the crate prefix and show just the
  // folder ("/domain"); otherwise keep the full key so cross-crate neighbours
  // stay distinguishable.
  const crateOfKey  = k => { const i = k.indexOf('/'); return i >= 0 ? k.slice(0, i) : k; };
  const drillCrate  = crateOfKey(drillGroup);
  const neighbourKeys = [...inGrpFiles.keys(), ...outGrpFiles.keys()];
  const singleCrate = neighbourKeys.every(k => crateOfKey(k) === drillCrate);
  const neighborLabel = k => {
    if (!singleCrate) return k;
    const i = k.indexOf('/');
    return i >= 0 ? '/' + k.slice(i + 1) : k;
  };

  const IN_EDGE_COLOR  = '#88bb88';
  const OUT_EDGE_COLOR = '#ccaa77';
  const IN_FILL        = '#edf7ed';
  const OUT_FILL       = '#fdf3e3';

  // Node style for external group boxes in the neighbor clusters
  // Always boxes regardless of metric mode — fixedsize/width from global node default must be reset.
  const extNode = (label, borderColor, fillColor) =>
    `[label=${dotId(label)} fillcolor="${fillColor}" color="${borderColor}" shape=box style=filled fixedsize=false fontname="Helvetica" fontsize=11]`;
  const inNodeId  = g => 'IN\x01' + g;
  const outNodeId = g => 'OUT\x01' + g;

  // Left cluster — callers of this group
  if (inGrpFiles.size > 0) {
    dot += `  subgraph cluster_in {\n`;
    dot += `    label="callers" style=filled fillcolor="${IN_FILL}" color="#88bb88" fontcolor="#447744" fontname="Helvetica" fontsize=11\n`;
    for (const g of inGrpFiles.keys())
      dot += `    ${dotId(inNodeId(g))} ${extNode(neighborLabel(g), IN_EDGE_COLOR, IN_FILL)}\n`;
    dot += '  }\n';
  }

  // Sub-clusters by directory within the drilled group. Labels are the full
  // workspace-relative directory path with a leading slash (e.g.
  // "/libs/modkit-odata-macros/src"), so the folder reads in full, not just its
  // crate-relative tail.
  const dirOf = n => nodeFullDir(n);
  const subGroups = new Map();
  drillNodes.forEach(n => { const d = dirOf(n); (subGroups.get(d) || subGroups.set(d, []).get(d)).push(n); });
  let si = 0;
  for (const [label, ns] of subGroups) {
    dot += `  subgraph cluster_${si++} {\n`;
    dot += `    label=${dotId(label)} color="#cccccc" fontcolor="#666666" fontname="Helvetica" fontsize=11\n`;
    for (const n of ns) dot += `    ${dotId(n.id)} [${nAttr(n)}]\n`;
    dot += '  }\n';
  }

  // Right cluster — dependencies of this group
  if (outGrpFiles.size > 0) {
    dot += `  subgraph cluster_out {\n`;
    dot += `    label="dependencies" style=filled fillcolor="${OUT_FILL}" color="#ccaa77" fontcolor="#886633" fontname="Helvetica" fontsize=11\n`;
    for (const g of outGrpFiles.keys())
      dot += `    ${dotId(outNodeId(g))} ${extNode(neighborLabel(g), OUT_EDGE_COLOR, OUT_FILL)}\n`;
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
    if (!edgeIsFlow(level, e.kind)) continue;   // map shows only flow connections
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
      dot += `  ${src} -> ${dotId(fid)} [color="${IN_EDGE_COLOR}" style="solid" constraint=false class="edge-in"]\n`;
    // If this group is also an outbound group (both roles), draw those edges too
    if (outGrpFiles.has(g)) {
      for (const fid of outGrpFiles.get(g))
        dot += `  ${dotId(fid)} -> ${src} [color="${IN_EDGE_COLOR}" style="solid" constraint=false class="edge-in"]\n`;
    }
  }
  // Our file → outbound group
  for (const [g, files] of outGrpFiles) {
    const tgt = dotId(outNodeId(g));
    for (const fid of files)
      dot += `  ${dotId(fid)} -> ${tgt} [color="${OUT_EDGE_COLOR}" style="solid" constraint=false class="edge-out"]\n`;
  }

  dot += '}';
  return dot;
}
