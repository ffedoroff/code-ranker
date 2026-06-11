// modal-content.js — builds the left field-table HTML of the node modal
// (buildModalContent). Consumes source-links.js and node-popup.js. Split out of
// diagram.js.

// Breadcrumb trail for a node's location, in the same chip style as the map's
// navigation breadcrumb (`.drill-crumb`/`.drill-sep`/`.crumb-tier`): the tier
// anchor, the `all`/`root` element, the crate/folder path chips, then the file
// itself (current). Folder/root chips are clickable (`data-mc-*`) — the modal's
// header click handler drills the map there and closes the modal.
function nodeCrumbsHtml(node, level) {
  if (isExternalNode(node, level)) return `<span class="nm-title">${escHtml(node.name)}</span>`;
  const tier      = window.viewTier(level);
  const rootLabel = tier === 'file' ? 'root' : 'all';
  // Tier dropdown — identical to the map breadcrumb (shared helper).
  const parts = [
    `<span class="crumb-tier">${window.tierAnchorHtml(level, tier)}</span>`,
    `<span class="drill-sep">›</span>`,
    `<button class="drill-crumb" data-mc-root="1" type="button" title="Show the whole overview">${rootLabel}</button>`,
  ];
  const tgt = focusFolderTarget(level, node);   // the file's containing-folder key + dig
  if (tgt.key && tgt.key !== '_root') {
    const segs = String(tgt.key).split('/');
    for (let i = 0; i < segs.length; i++) {
      const key = segs.slice(0, i + 1).join('/');
      const dg  = chipDig(level, i, tier);
      parts.push('<span class="drill-sep">›</span>');
      parts.push(`<button class="drill-crumb" data-mc-key="${escAttr(key)}" data-mc-dig="${dg}" type="button">${escHtml(segs[i])}</button>`);
    }
  }
  parts.push('<span class="drill-sep">›</span>');
  parts.push(`<span class="drill-crumb-cur nm-crumb-file">${escHtml(node.name)}</span>`);
  return `<span class="nm-crumbs">${parts.join(' ')}</span>`;
}

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

  const tipAttr = escAttr;

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

  const esc = escAttr;

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

  return {
    hdr:      nodeHeaderHtml(node, level),
    body,
    diagram:  buildDiagramSVG(node, level),
  };
}

// The modal header markup: the file breadcrumb + kind badge + active-side badge.
// Reused to re-render the header in place when the tier dropdown switches the
// representation (without rebuilding the whole modal / losing the open file).
function nodeHeaderHtml(node, level) {
  const sideSuffix = (typeof viewModeSuffix === 'function') ? viewModeSuffix().trim() : '';
  return nodeCrumbsHtml(node, level) + `<span class="nm-badge">${escHtml(node.kind)}</span>` +
         (sideSuffix ? `<span class="nm-side">${escHtml(sideSuffix)}</span>` : '');
}
window.nodeHeaderHtml = nodeHeaderHtml;
