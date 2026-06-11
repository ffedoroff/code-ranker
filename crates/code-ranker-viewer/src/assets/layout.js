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

    // Crate-tier groups (zoom 0) are pink; any other grouping (folders, or the
    // file tier) is a uniform neutral white, so the colour signals "these are crates".
    const isCrateTier = window.viewTier(level) === 'crate' && activeDig === 0 && !!(levelUi(level).grouping?.key);
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
    const clusterByCrate = window.viewTier(level) === 'crate' && activeDig > 0 && !!(levelUi(level).grouping?.key);
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
    // Non-flow inter-group edges (contains / reexports): dashed + hidden until a
    // group hover reveals them; skip pairs already linked by a flow edge.
    const seenGroupNF = new Set();
    for (const e of edges) {
      if (edgeIsFlow(level, e.kind)) continue;
      const sg = nodeGroup.get(e.source);
      const tg = nodeGroup.get(e.target);
      if (!sg || !tg || sg === tg) continue;
      const key = sg + '\x00' + tg;
      if (seenGroupEdge.has(key) || seenGroupNF.has(key)) continue;
      seenGroupNF.add(key);
      dot += `  ${dotId(sg)} -> ${dotId(tg)} [color="${E_COLOR}" style="dashed" constraint=false class="edge-nonflow"]\n`;
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

  // ── Focus level-of-detail (`window.focusDig`) ─────────────────────────────────
  // 0 = individual files (default); a negative value collapses the focus's files
  // into folder boxes, deepest folders first (mirrors the overview's dig out → in).
  const underDepth = n => underDepthOf(level, n);   // shared (see map-interactions.js)
  const maxFocusD  = drillNodes.length ? Math.max(...drillNodes.map(underDepth)) : 0;
  const fz         = window.focusDig || 0;
  // Reveal depth D (0 = the most-collapsed landing, up to maxRel = all files). A
  // node is shown as an individual FILE when it sits at or above the revealed
  // frontier (its folder level under the focus ≤ D); deeper nodes collapse into a
  // folder box at the frontier (focus + D + 1 levels). So depth 0 shows the focus's
  // direct files (in their dir cluster) plus its immediate subfolders as boxes.
  const minFz      = -Math.max(0, maxFocusD - activeDig);
  const D          = fz - minFz;
  const frontierDig = activeDig + D + 1;
  // The focus's PARENT dir — subtracted from folder labels so a drilled view shows
  // paths relative to where you are while keeping the focus folder's own name
  // (focus `…/sdk/src` → `/src`, children `/src/render`), not the long ancestor path.
  const focusBase  = focusStripBase(level);
  const relLevel   = n => underDepth(n) - activeDig;
  const isFileNode = n => relLevel(n) <= D;
  const renderId   = id => { const n = allNodesById.get(id); return (n && !isFileNode(n)) ? groupKeyAtDig(level, n, frontierDig) : id; };
  const anyBoxed   = drillNodes.some(n => !isFileNode(n));
  // _FOCUS.focusD is the dig the collapsed folder boxes are keyed at (for the
  // tooltip/click handlers); folderMode flags that some boxes are present.
  window._FOCUS = { folderMode: anyBoxed, focusD: frontierDig, maxFocusD };

  const layoutDiam = n => {
    const db = baselineById.has(n.id) ? metricNodeDiam(baselineById.get(n.id), sizeMode) : 0;
    const da = currentById.has(n.id)  ? metricNodeDiam(currentById.get(n.id),  sizeMode) : 0;
    return Math.max(db, da) || metricNodeDiam(n, sizeMode);
  };

  const edgeCycleOf = window.CYCLES?.[level]?.edgeCycleStatus;
  // Non-flow edges (contains / reexports) render DASHED and tagged `edge-nonflow`
  // so CSS keeps them hidden until a node hover reveals the connected ones; flow
  // edges stay solid and always visible.
  const eAttr = e => {
    const flow = edgeIsFlow(level, e.kind);
    return `color="${E_COLOR}" style="${flow ? 'solid' : 'dashed'}" class="edge-${e.kind || 'unknown'} status-${e.status} cycle-status-${edgeCycleOf ? edgeCycleOf(e.source, e.target) : 'none'}${flow ? '' : ' edge-nonflow'}"`;
  };

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

  // ── Collect neighbour CRATES (callers / dependencies, no 3rd-party) ───────────
  // Group every cross-boundary edge by the OTHER end's **crate** (regardless of
  // tier / focus depth), so the boxes are a stable list of crates. Both **flow**
  // (uses) and **non-flow** (contains / reexports) edges are included. Per crate:
  //   • `their` — the distinct neighbour-side files coupled via **flow** edges (the
  //     box's `(N)` count); a crate reached only by non-flow edges counts `(0)`.
  //   • `our`   — our render-ids the connector edges fan to, with per-diff-side
  //     presence (Baseline/Current toggle) and `flow` = does ANY edge to that file
  //     flow (flow wins → solid connector; else dashed).
  // A crate that is both a caller and a dependency appears on the left only.
  const crateOf = n => crateIdOf(level, n) ?? gOf(n);
  const inGrp  = new Map();   // crate → { their:Set<flow-their-file>, our:Map<our-id,{b,c,flow}> }
  const outGrp = new Map();
  const touch = (m, crate, theirFile, ourId, e, flow) => {
    let r = m.get(crate);
    if (!r) { r = { their: new Set(), our: new Map() }; m.set(crate, r); }
    if (flow) r.their.add(theirFile);   // count flow-coupled files only
    let rec = r.our.get(ourId);
    if (!rec) { rec = { b: false, c: false, flow: false }; r.our.set(ourId, rec); }
    rec.b = rec.b || e.status !== 'added';    // present in baseline
    rec.c = rec.c || e.status !== 'removed';  // present in current
    rec.flow = rec.flow || flow;              // flow priority: solid if any flow edge
  };
  for (const e of edges) {
    const flow = edgeIsFlow(level, e.kind);
    const sIn = drillIds.has(e.source), tIn = drillIds.has(e.target);
    if (!sIn && tIn) {
      const src = allNodesById.get(e.source);
      if (!src || isExternalNode(src, level)) continue;
      touch(inGrp, crateOf(src), e.source, renderId(e.target), e, flow);
    } else if (sIn && !tIn) {
      const tgt = allNodesById.get(e.target);
      if (!tgt || isExternalNode(tgt, level)) continue;
      touch(outGrp, crateOf(tgt), e.target, renderId(e.source), e, flow);
    }
  }
  for (const c of inGrp.keys()) outGrp.delete(c);   // a crate in both → callers only

  // Diff side-presence → the same status class the union nodes/edges carry, so the
  // `.hide-{nodes,edges}-{added,removed}` toggle CSS hides them on the off side.
  const statusClass = (b, c) => (b && c) ? 'unchanged' : c ? 'added' : 'removed';
  const grpStatus = r => { let b = false, c = false; for (const rec of r.our.values()) { b = b || rec.b; c = c || rec.c; } return statusClass(b, c); };
  // Whole-cluster status (callers / dependencies): OR over every connection in it,
  // so the cluster background+label hides on the side where it has none (member
  // boxes are siblings of the cluster <g> in graphviz SVG, so hiding the <g> only
  // hides its background/label, not the boxes — hence the per-box status above).
  const clusterStatus = m => { let b = false, c = false; for (const r of m.values()) for (const rec of r.our.values()) { b = b || rec.b; c = c || rec.c; } return statusClass(b, c); };

  const IN_EDGE_COLOR  = '#88bb88';
  const OUT_EDGE_COLOR = '#ccaa77';
  const IN_FILL        = '#edf7ed';
  const OUT_FILL       = '#fdf3e3';

  // Node style for external group boxes in the neighbor clusters
  // Always boxes regardless of metric mode — fixedsize/width from global node default must be reset.
  // `dashed` → a crate reached only by non-flow edges gets a dashed outline.
  const extNode = (label, borderColor, fillColor, cls, dashed) =>
    `[label=${dotId(label)} fillcolor="${fillColor}" color="${borderColor}" shape=box style="${dashed ? 'filled,dashed' : 'filled'}" fixedsize=false fontname="Helvetica" fontsize=11${cls ? ` class="${cls}"` : ''}]`;
  const inNodeId  = g => 'IN\x01' + g;
  const outNodeId = g => 'OUT\x01' + g;

  // Left cluster — caller crates (label `crate (N coupled files)`)
  if (inGrp.size > 0) {
    dot += `  subgraph cluster_in {\n`;
    dot += `    class="status-${clusterStatus(inGrp)}"\n`;
    dot += `    label="callers" style=filled fillcolor="${IN_FILL}" color="#88bb88" fontcolor="#447744" fontname="Helvetica" fontsize=11\n`;
    for (const [crate, r] of inGrp)
      dot += `    ${dotId(inNodeId(crate))} ${extNode(`${crate} (${r.their.size})`, IN_EDGE_COLOR, IN_FILL, 'status-' + grpStatus(r), r.their.size === 0)}\n`;
    dot += '  }\n';
  }

  // Reveal frontier: nodes at/above depth D render as individual files inside
  // their directory sub-cluster; deeper nodes collapse into a folder box at the
  // frontier. Both kinds can appear together — e.g. the focus's direct files in a
  // "/src" cluster alongside collapsed "/src/render", "/src/scan" boxes.
  const fileNodes = drillNodes.filter(isFileNode);
  const boxNodes  = drillNodes.filter(n => !isFileNode(n));

  // Collapsed folder boxes (deeper than the frontier), deduped by box key.
  const boxes = new Map();
  for (const n of boxNodes) { const k = groupKeyAtDig(level, n, frontierDig); (boxes.get(k) || boxes.set(k, []).get(k)).push(n); }
  for (const [k, ns] of boxes) {
    const gCyc = aggCycleStatus(ns.map(n => cycleOf?.get(n.id) || 'none'));
    const lbl  = `${stripDirPrefix(focusBase, groupLabel(level, k, frontierDig))} (${ns.length})`;
    // Collapsed folders are grey (matching the expanded dir sub-clusters) so they
    // read as folders, distinct from the blue file nodes.
    dot += `  ${dotId(k)} [label=${dotId(lbl)} fillcolor="#ececec" color="#bbbbbb" fontcolor="#555555" shape=box style=filled fontname="Helvetica" fontsize=11 class="cycle-status-${gCyc}"]\n`;
  }

  // Revealed files: directory sub-clusters labelled with the full workspace-relative
  // path (e.g. "/libs/modkit-odata-macros/src"), faint-filled so the folder area is
  // hoverable/clickable to drill in.
  const subGroups = new Map();
  fileNodes.forEach(n => { const d = nodeFullDir(n); (subGroups.get(d) || subGroups.set(d, []).get(d)).push(n); });
  let si = 0;
  for (const [label, ns] of subGroups) {
    dot += `  subgraph cluster_${si++} {\n`;
    dot += `    label=${dotId(stripDirPrefix(focusBase, label))} style=filled fillcolor="#f7f7f7" color="#cccccc" fontcolor="#666666" fontname="Helvetica" fontsize=11\n`;
    for (const n of ns) dot += `    ${dotId(n.id)} [${nAttr(n)}]\n`;
    dot += '  }\n';
  }

  // Right cluster — dependency crates (label `crate (N coupled files)`)
  if (outGrp.size > 0) {
    dot += `  subgraph cluster_out {\n`;
    dot += `    class="status-${clusterStatus(outGrp)}"\n`;
    dot += `    label="dependencies" style=filled fillcolor="${OUT_FILL}" color="#ccaa77" fontcolor="#886633" fontname="Helvetica" fontsize=11\n`;
    for (const [crate, r] of outGrp)
      dot += `    ${dotId(outNodeId(crate))} ${extNode(`${crate} (${r.their.size})`, OUT_EDGE_COLOR, OUT_FILL, 'status-' + grpStatus(r), r.their.size === 0)}\n`;
    dot += '  }\n';
  }

  // Pin callers strictly left, dependencies strictly right
  if (inGrp.size > 0) {
    dot += '  { rank=min';
    for (const c of inGrp.keys()) dot += `; ${dotId(inNodeId(c))}`;
    dot += ' }\n';
  }
  if (outGrp.size > 0) {
    dot += '  { rank=max';
    for (const c of outGrp.keys()) dot += `; ${dotId(outNodeId(c))}`;
    dot += ' }\n';
  }

  // ── Edges ─────────────────────────────────────────────────────────────────────
  // Internal edges (within the drilled group). Flow edges (solid) are laid out
  // normally; non-flow edges (contains / reexports) are added DASHED with
  // `constraint=false` (so they don't distort the layout) and hidden by CSS until
  // a node hover reveals the connected ones. A non-flow pair already linked by a
  // flow edge is skipped to avoid a doubled line.
  const flowPairs = new Set();
  for (const e of edges) {
    if (!edgeIsFlow(level, e.kind)) continue;
    if (!drillIds.has(e.source) || !drillIds.has(e.target)) continue;
    const s = renderId(e.source), t = renderId(e.target);
    if (s === t) continue;   // collapsed into the same folder box
    const key = s + '\x00' + t;
    if (flowPairs.has(key)) continue;
    flowPairs.add(key);
    dot += `  ${dotId(s)} -> ${dotId(t)} [${eAttr(e)}]\n`;
  }
  const seenNonFlow = new Set();
  for (const e of edges) {
    if (edgeIsFlow(level, e.kind)) continue;
    if (!drillIds.has(e.source) || !drillIds.has(e.target)) continue;
    const s = renderId(e.source), t = renderId(e.target);
    if (s === t) continue;
    const key = s + '\x00' + t;
    if (flowPairs.has(key) || seenNonFlow.has(key)) continue;
    seenNonFlow.add(key);
    dot += `  ${dotId(s)} -> ${dotId(t)} [${eAttr(e)} constraint=false]\n`;
  }

  // Caller crate → our files (one connector per coupled our-file). The `status-*`
  // class makes the connector follow the Baseline/Current toggle just like the
  // internal edges — a link that exists in only one snapshot hides on the other.
  for (const [crate, r] of inGrp) {
    const src = dotId(inNodeId(crate));
    for (const [fid, rec] of r.our)
      dot += `  ${src} -> ${dotId(fid)} [color="${IN_EDGE_COLOR}" style="${rec.flow ? 'solid' : 'dashed'}" constraint=false class="edge-in status-${statusClass(rec.b, rec.c)}"]\n`;
  }
  // Our files → dependency crate
  for (const [crate, r] of outGrp) {
    const tgt = dotId(outNodeId(crate));
    for (const [fid, rec] of r.our)
      dot += `  ${dotId(fid)} -> ${tgt} [color="${OUT_EDGE_COLOR}" style="${rec.flow ? 'solid' : 'dashed'}" constraint=false class="edge-out status-${statusClass(rec.b, rec.c)}"]\n`;
  }

  dot += '}';
  return dot;
}
