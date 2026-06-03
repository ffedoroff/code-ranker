// schema.js — the ONLY place that knows the snapshot JSON shape.
//
// The viewer is a pure renderer: every metric label, description, formula,
// threshold, colour, preset and prompt comes from the snapshot's per-level
// dictionaries (node_attributes / edge_attributes / edge_kinds /
// attribute_groups / node_kinds / cycle_kinds / ui) and the top-level
// `presets`. No metric/kind is hardcoded by name anywhere in the frontend.

// The level's dictionaries (specs) — read from the active snapshot, which is the
// authority for how to render. Falls back to the other side so a single-snapshot
// report works.
function specSnap() {
  return (window.viewSide === 'before')
    ? (window.BEFORE ?? window.AFTER)
    : (window.AFTER ?? window.BEFORE);
}
function levelSpec(level) {
  return specSnap()?.graphs?.[level] || {};
}

// ── Attribute specs ──────────────────────────────────────────────────────────
function attrSpec(level, key)  { return levelSpec(level).node_attributes?.[key] || {}; }
function edgeAttrSpec(level, k) { return levelSpec(level).edge_attributes?.[k] || {}; }
function attrLabel(level, key)  { const s = attrSpec(level, key); return s.label || key; }
// Full name for a tooltip title (falls back to label, then key).
function attrName(level, key)   { const s = attrSpec(level, key); return s.name || s.label || key; }
// Short label for narrow table headers (falls back to label, then key).
function attrShort(level, key)  { const s = attrSpec(level, key); return s.short || s.label || key; }
function attrDesc(level, key)   { return attrSpec(level, key).description || ''; }
function attrFormula(level, key){ return attrSpec(level, key).formula || ''; }
function attrType(level, key)   { return attrSpec(level, key).value_type || 'str'; }
function attrGroup(level, key)  { return attrSpec(level, key).group || null; }
function attrThresholds(level, key) { return attrSpec(level, key).thresholds || null; }
function attrAbbrev(level, key) { return attrSpec(level, key).abbreviate === true; }
// 'lower_better' / 'higher_better' / null — drives delta colouring.
function attrDirection(level, key) { return attrSpec(level, key).direction || null; }

// All numeric attribute keys declared at a level, in spec (alphabetical) order.
function numericAttrKeys(level) {
  const na = levelSpec(level).node_attributes || {};
  return Object.keys(na).filter(k => na[k].value_type === 'int' || na[k].value_type === 'float');
}

// ── Node kinds ────────────────────────────────────────────────────────────────
function nodeKindSpec(level, kind) { return levelSpec(level).node_kinds?.[kind] || {}; }
function isExternalNode(node, level) {
  if (!node) return false;
  if (node.external === true) return true;                 // explicit attr
  return nodeKindSpec(level, node.kind).external === true;  // by kind spec
}
// Ids of every external node in a graph (level passed for the kind dictionary).
function externalIdSet(graph, level) {
  return new Set((graph?.nodes || []).filter(n => isExternalNode(n, level)).map(n => n.id));
}

// ── Edge kinds ──────────────────────────────────────────────────────────────
// Is this edge kind information flow (drawn + counted)? Unknown kinds default to
// flow. `contains`-style structural kinds carry flow:false in the dictionary.
function edgeIsFlow(level, kind) {
  const s = levelSpec(level).edge_kinds?.[kind];
  return s ? s.flow !== false : true;
}
function edgeKindLabel(level, kind) { return levelSpec(level).edge_kinds?.[kind]?.label || kind; }
function edgeKindDesc(level, kind)  { return levelSpec(level).edge_kinds?.[kind]?.description || ''; }

// ── Cycles ────────────────────────────────────────────────────────────────────
function cycleKindLabel(level, kind) { return levelSpec(level).cycle_kinds?.[kind]?.label || kind; }
function cycleKindDesc(level, kind)  { return levelSpec(level).cycle_kinds?.[kind]?.description || ''; }

// ── UI hints / groups / presets ────────────────────────────────────────────
function levelUi(level)        { return levelSpec(level).ui || {}; }
function attributeGroups(level){ return levelSpec(level).attribute_groups || {}; }
function snapshotPresets()     { return specSnap()?.presets || []; }

// ── Live metric computation (eval) ─────────────────────────────────────────
// Build a function from a `calc` expression (bare attribute names + Math) and
// run it against a node's flat numeric attributes. Returns null on any error.
function evalCalc(calc, node, keys) {
  if (!calc) return null;
  try {
    const vals = keys.map(k => Number(node[k] ?? 0));
    // eslint-disable-next-line no-new-func
    const fn = Function('Math', ...keys, `return (${calc});`);
    const r = fn(Math, ...vals);
    return typeof r === 'number' && isFinite(r) ? r : null;
  } catch {
    return null;
  }
}

// A small number formatter for substituted operands / results (no abbreviation —
// the substituted formula reads like the metric's own value).
function calcNum(v) {
  if (typeof v !== 'number' || !isFinite(v)) return String(v);
  if (v === Math.round(v)) return String(v);
  return String(Math.round(v * 1000) / 1000);
}

// "formula with this node's numbers = result" — the live derivation line shown
// in a metric's tooltip. Substitutes attribute-name tokens in the *pretty*
// `formula` with the node's values (only where the attr is a real key), then
// appends the eval'd result. Returns '' when the metric has no `calc`.
function calcDisplay(level, key, node) {
  const spec = attrSpec(level, key);
  if (!spec.calc) return '';
  const keys = numericAttrKeys(level);
  const result = evalCalc(spec.calc, node, keys);
  if (result === null) return '';
  let shown = spec.formula || spec.calc;
  // Replace whole-word attribute tokens with the node's value. snake_case keys
  // are safe: '\bmi\b' won't match inside 'mi_sei' (the '_' is a word char).
  for (const k of keys) {
    if (shown.includes(k)) {
      shown = shown.replace(new RegExp(`\\b${k}\\b`, 'g'), calcNum(Number(node[k] ?? 0)));
    }
  }
  return `${shown} = ${calcNum(result)}`;
}

// Read a node's value for a numeric/text attribute (flat in v2). null when absent.
function nodeAttr(node, key) {
  const v = node?.[key];
  return v === undefined ? null : v;
}
