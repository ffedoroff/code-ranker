// modal-content.js — builds the left field-table HTML of the node modal
// (buildModalContent). Consumes source-links.js and node-popup.js. Split out of
// diagram.js.

function buildModalContent(node, level) {
  const cycles  = window.CYCLES?.[level];
  const cs      = cycles?.nodeCycleStatus?.get(node.id);
  const mnExt   = isExternalNode(node, level);
  // Displayed path: external keeps its compact `{registry}`/`{cargo}` token
  // form; for project files the id IS the relativized path (the `path` attr is
  // dropped when equal to the id), so fall back to the id, then drop the leading
  // root token to get the repo-relative path.
  const path    = mnExt ? (node.path || node.id || '')
                        : (node.path || node.id || '').replace(/^\{[^}]+\}\//, '');
  // Absolute on-disk path (token expanded) for the Path-row tooltip.
  const absFull = absPath(mnExt ? (node.path || node.id) : node.id);
  const vis     = typeof node.visibility === 'string' ? node.visibility : null;

  // sections: array of { label: string|null, rows: string[] }
  const sections = [];
  let cur = { label: null, rows: [] };

  const tipAttr = s => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;');

  // Build a field row. `key` is the attribute key; `v` is the formatted value
  // string; optional `calc` is the live derivation line.
  const row = (key, v, opts) => {
    if (v == null || v === '') return;
    const label   = attrLabel(level, key) || (key.charAt(0).toUpperCase() + key.slice(1));
    const title   = attrName(level, key)  || label;
    const desc    = attrDesc(level, key);
    const formula = attrFormula(level, key);
    const calc    = opts?.calc || '';
    const attr = desc
      ? ` data-tip="${tipAttr(desc)}" data-tip-title="${tipAttr(title)}"` +
        (formula ? ` data-tip-formula="${tipAttr(formula)}"` : '') +
        (calc    ? ` data-tip-calc="${tipAttr(calc)}"` : '')
      : '';
    cur.rows.push(`<tr${attr}><td class="nm-key">${label}</td><td class="nm-val">${v}</td></tr>`);
  };

  // A plain row with no schema lookup (for id, path, source — structural fields).
  const rawRow = (label, valHtml, tipTitle, tipDesc) => {
    const attr = tipDesc
      ? ` data-tip="${tipAttr(tipDesc)}" data-tip-title="${tipAttr(tipTitle || label)}"`
      : '';
    cur.rows.push(`<tr${attr}><td class="nm-key">${label}</td><td class="nm-val">${valHtml}</td></tr>`);
  };

  const sect = label => { sections.push(cur); cur = { label, rows: [] }; };

  const esc = s => String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');

  // The node popup is roomy, so it shows every value VERBATIM (`fmtFull` in
  // utils.js — no rounding or abbreviation, just thousands separators).

  // ── Structural fields ─────────────────────────────────────────────────────

  if (path) {
    const si   = path.lastIndexOf('/');
    const dir  = si >= 0 ? esc(path.slice(0, si + 1)) : '';
    const file = esc(si >= 0 ? path.slice(si + 1) : path);
    rawRow('Path',
      `${dir}<strong>${file}</strong>`,
      attrName(level, 'path') || 'Path',
      absFull || attrDesc(level, 'path') || 'Location of this node.'
    );
    // Source link for project files (not for external nodes).
    if (!mnExt) {
      const url = nodeSourceUrl(node, level);
      if (url) {
        const host = url.replace(/^https?:\/\//i, '').split('/')[0];
        cur.rows.push(
          `<tr><td class="nm-key">Source</td><td class="nm-val">` +
          `<a class="nm-src" href="${esc(url)}" target="_blank" rel="noopener noreferrer">${esc(host)} ↗</a>` +
          `</td></tr>`
        );
      }
    }
  }

  // id for external nodes
  if (mnExt) row('id', node.id);
  row('kind', node.kind || null);
  row('version', node.version ?? null);
  if (mnExt) row('external', 'true');
  // visibility: only when present and not "public"
  if (vis && vis !== 'public') row('visibility', vis);
  if (node.items != null) row('items', fmtFull(node.items));
  // node.cycle is the cycle kind (mutual/chain/…); cs is the diff-side status
  // (both/baseline-only/current-only) computed at runtime from window.CYCLES.
  if (node.cycle != null) row('cycle', node.cycle);
  if (cs && cs !== 'none') rawRow('Cycle status', cs, 'Cycle status', 'Whether this cycle exists on the baseline side, current side, or both.');
  if (!document.body.classList.contains('mode-review')) row('status', node.status);

  // ── Numeric metric sections, driven by numericAttrKeys + attribute_groups ─

  // Group keys by their `group` field (preserving declaration order).
  const numKeys = numericAttrKeys(level);
  const groups  = attributeGroups(level);  // { id: { label, description } }

  // Collect keys that have a non-null value on this node, grouped.
  const grouped = {};   // groupId → [key, ...]
  const ungrouped = []; // keys with no group
  for (const k of numKeys) {
    const v = nodeAttr(node, k);
    if (v == null) continue;
    const g = attrGroup(level, k);
    if (g) {
      if (!grouped[g]) grouped[g] = [];
      grouped[g].push(k);
    } else {
      ungrouped.push(k);
    }
  }

  // Emit ungrouped numeric keys first (no section header).
  if (ungrouped.length > 0) {
    sect(null);
    for (const k of ungrouped) {
      const v = nodeAttr(node, k);
      row(k, fmtFull(v), { calc: calcDisplay(level, k, node) });
    }
  }

  // Emit each group in the order they appear in attribute_groups.
  const groupOrder = Object.keys(groups);
  // Emit groups that appear in attribute_groups first, then any remaining.
  const allGroupIds = [
    ...groupOrder.filter(g => grouped[g]),
    ...Object.keys(grouped).filter(g => !groupOrder.includes(g)),
  ];

  for (const gId of allGroupIds) {
    const keys = grouped[gId];
    if (!keys || keys.length === 0) continue;
    const gLabel = groups[gId]?.label || gId;
    sect(gLabel);
    for (const k of keys) {
      const v = nodeAttr(node, k);
      row(k, fmtFull(v), { calc: calcDisplay(level, k, node) });
    }
  }

  sections.push(cur);

  const renderSect = s =>
    `${s.label ? `<div class="nm-sect-label">${s.label}</div>` : ''}` +
    `<table class="nm-table">${s.rows.join('')}</table>`;

  const body = sections.filter(s => s.rows.length > 0).map(renderSect).join('');

  const sideSuffix = (typeof viewModeSuffix === 'function') ? viewModeSuffix().trim() : '';
  return {
    hdr:      `<span class="nm-title">${node.name}</span><span class="nm-badge">${node.kind}</span>` +
              (sideSuffix ? `<span class="nm-side">${sideSuffix}</span>` : ''),
    body,
    diagram:  buildDiagramSVG(node, level),
  };
}
