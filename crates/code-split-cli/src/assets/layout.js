const N_FILL  = '#dbe9f4';
const N_COLOR = '#4d6f9c';
const E_COLOR = '#4d6f9c';
// External (3rd-party library) nodes/edges are drawn in a distinct amber.
const EXT_FILL  = '#f6e2c0';
const EXT_COLOR = '#b3801f';

function dotId(id) {
  return '"' + id.replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"';
}

function buildDOT(nodes, edges, level) {
  const cycles = window.CYCLES?.[level];

  // ── Node sizing: default=boxes+labels, loc/hk=circles sized by metric ────
  const mode      = window.nodeSizeMode || 'default';
  const getLoc    = n => n.complexity?.loc?.source ?? (typeof n.loc === 'number' ? n.loc : 0);
  const BASE_DIAM = 0.3, BASE_LOC = 100, BASE_HK = 1000;

  const fmtShort = v => {
    if (v >= 1_000_000) return (Math.round(v / 100_000) / 10) + 'M';
    if (v >= 10_000)    return Math.round(v / 1_000) + 'K';
    if (v >= 1_000)     return (Math.round(v / 100) / 10) + 'K';
    return String(Math.round(v));
  };

  const nodeVal = n => {
    if (mode === 'loc') return getLoc(n);
    if (mode === 'hk')  return n.complexity?.coupling?.hk ?? 0;
    return 0;
  };

  const nodeDiam = n => {
    const v = nodeVal(n);
    if (mode === 'loc') return +(BASE_DIAM * Math.sqrt(Math.max(v, BASE_LOC) / BASE_LOC)).toFixed(3);
    if (mode === 'hk')  return v === 0 ? 0.3 : +(BASE_DIAM * Math.sqrt(Math.max(v, BASE_HK) / BASE_HK)).toFixed(3);
    return 0.3;
  };

  const isExt = n => n.kind === 'external' || n.external === true;
  const nAttr = n => {
    const cs   = cycles?.nodeCycleStatus?.get(n.id) || 'none';
    const ext  = isExt(n);
    const fill = ext ? EXT_FILL : N_FILL;
    const col  = ext ? EXT_COLOR : N_COLOR;
    const cls  = `class="node-${n.kind || 'unknown'} status-${n.status} cycle-status-${cs}"`;
    if (mode === 'default') {
      return `label=${dotId(n.name)} fillcolor="${fill}" color="${col}" ${cls}`;
    }
    const d   = nodeDiam(n);
    const v   = nodeVal(n);
    const lbl = v > 0 ? fmtShort(v) : '';
    const fs  = Math.max(6, Math.round(d * 26));
    return `label=${dotId(lbl)} fontsize=${fs} fontcolor="#333" fillcolor="${fill}" color="${col}" width=${d} ${cls}`;
  };
  const eAttr = e => {
    const cs  = cycles?.edgeCycleStatus?.(e.from, e.to) || 'none';
    const col = e.external === true ? EXT_COLOR : E_COLOR;
    const style = e.external === true ? 'dashed' : 'solid';
    return `color="${col}" style="${style}" class="edge-${e.kind || 'unknown'} status-${e.status} cycle-status-${cs}"`;
  };

  let dot = 'digraph {\n';
  dot += '  rankdir=LR\n';
  dot += '  graph [bgcolor="white" fontname="Helvetica" pad="0.5" nodesep="0.25" ranksep="1.0"]\n';
  if (mode === 'default') {
    dot += '  node  [shape=box style=filled fontname="Helvetica" fontsize=11]\n\n';
  } else {
    dot += '  node  [shape=circle style=filled fixedsize=true width=0.3]\n\n';
  }

  // Single file graph: cluster file nodes by directory; external library nodes
  // (no path) go into a dedicated "libraries" cluster.
  const dirOf = n => {
    if (isExt(n)) return 'libraries';
    const p = (n.path || n.id).replace(/^\{[^}]+\}\//, '').replace(/^file:/, '');
    const i = p.lastIndexOf('/');
    return i > 0 ? p.slice(0, i) : '_root';
  };
  const dirs = new Map();
  nodes.forEach(n => { const d = dirOf(n); (dirs.get(d) || dirs.set(d, []).get(d)).push(n); });
  let i = 0;
  for (const [dir, ns] of dirs) {
    const label = dir === 'libraries' ? 'libraries' : dir.split('/').slice(-2).join('/');
    dot += `  subgraph cluster_${i++} {\n`;
    dot += `    label=${dotId(label)} color="#cccccc" fontcolor="#666666"\n`;
    for (const n of ns) dot += `    ${dotId(n.id)} [${nAttr(n)}]\n`;
    dot += '  }\n';
  }

  for (const e of edges)
    dot += `  ${dotId(e.from)} -> ${dotId(e.to)} [${eAttr(e)}]\n`;

  dot += '}';
  return dot;
}
