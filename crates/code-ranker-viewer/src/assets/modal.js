// ── Fixed-position tooltip (no layout shift) ─────────────────────────────────
let _ttDiv = null;
function _getTooltip() {
  if (!_ttDiv) {
    _ttDiv = document.createElement('div');
    _ttDiv.id = 'nm-tooltip';
    document.body.appendChild(_ttDiv);
  }
  return _ttDiv;
}
function _positionTooltip(tt, anchor) {
  const r  = anchor.getBoundingClientRect();
  const tw = tt.offsetWidth, th = tt.offsetHeight;
  let left = r.right + 8;
  let top  = r.top - 4;
  if (left + tw > window.innerWidth  - 10) left = r.left - tw - 8;
  if (top  + th > window.innerHeight - 10) top  = window.innerHeight - th - 10;
  if (top < 10) top = 10;
  tt.style.left = left + 'px';
  tt.style.top  = top  + 'px';
}
document.addEventListener('click', e => {
  const btn = e.target.closest('.nm-copy-btn');
  if (!btn) return;
  navigator.clipboard.writeText(btn.dataset.copy ?? '').then(() => {
    const prev = btn.textContent;
    btn.textContent = '✓';
    setTimeout(() => { btn.textContent = prev; }, 1200);
  });
});

// The central (main) card mirrors the side cards under modifiers: ⌘/Ctrl-click
// views its source, Shift-click toggles its selection (routed through the modal
// checkbox so the row / map node / footer all stay in sync). A *plain* click
// (no modifier) copies its path — the SVG <g> feedback is a CSS class, not a
// textContent swap that would clobber the group's children. External main cards
// (`diag-ext`) are inert under modifiers.
document.addEventListener('click', e => {
  const card = e.target.closest('.mn-card');
  if (!card) return;

  const shift = e.shiftKey, src = window.isOpenSrcClick?.(e);
  if (shift || src) {
    if (!card.classList.contains('diag-ext')) {
      if (src) {
        const level  = document.querySelector('.view.active')?.dataset.view;
        const node   = card.dataset.nodeId && level
          ? window.activeGraph?.(level)?.nodes?.find(n => n.id === card.dataset.nodeId) : null;
        const url = node && window.nodeSourceUrl?.(node, level);
        if (url) window.open(url, '_blank', 'noopener');
      } else {  // Shift → toggle selection via the checkbox (full sync + live highlight)
        const cb = document.getElementById('node-modal-cb');
        if (cb) { cb.checked = !cb.checked; cb.dispatchEvent(new Event('change')); }
      }
    }
    return;   // a modifier click never copies
  }

  // Plain click copies only when it lands on a specific label (`.mn-copy` — the
  // title or a `key: value` row), each copying its own value. A click on the
  // bare card never copies.
  const lbl = e.target.closest('.mn-copy');
  if (!lbl || lbl.dataset.copy == null) return;
  const val = lbl.dataset.copy;
  navigator.clipboard.writeText(val).then(() => {
    const preview = card.querySelector('.mn-copied-val');
    if (preview) preview.textContent = val.length > 48 ? val.slice(0, 47) + '…' : val;
    card.classList.add('copied');
    setTimeout(() => card.classList.remove('copied'), 1000);
  });
});

document.addEventListener('mouseover', e => {
  const h = e.target.closest('.nm-has-hint');
  if (h && h.dataset.nmHint) {
    const tt = _getTooltip();
    tt.textContent = `${h.textContent.trim()}: ${h.dataset.nmHint}`;
    tt.style.display = 'block';
    requestAnimationFrame(() => _positionTooltip(tt, h));
  } else if (!e.target.closest('#nm-tooltip')) {
    if (_ttDiv) _ttDiv.style.display = 'none';
  }
});

function getModal() {
  let overlay = document.getElementById('node-modal-overlay');
  if (!overlay) {
    overlay = document.createElement('div');
    overlay.id = 'node-modal-overlay';
    overlay.innerHTML =
      '<div id="node-modal">' +
        '<div id="node-modal-hdr">' +
          '<div id="node-modal-hdr-title"></div>' +
          '<button id="node-modal-close" title="Close">✕</button>' +
        '</div>' +
        '<div id="node-modal-main">' +
          '<div id="node-modal-body"></div>' +
          '<div id="node-modal-diagram"></div>' +
        '</div>' +
      '</div>';
    document.body.appendChild(overlay);
    document.getElementById('node-modal-close').addEventListener('click', closeModal);
    overlay.addEventListener('mousedown', e => { if (e.target === overlay) closeModal(); });
    // Header breadcrumb chips: drill the map to that crate/folder and close the modal.
    document.getElementById('node-modal-hdr-title').addEventListener('click', e => {
      const level = window._modalNode?.level ?? document.querySelector('.view.active')?.dataset.view;
      // Tier dropdown: toggle the menu in place; picking a tier switches the map.
      if (window.handleTierToggle?.(e)) return;
      const opt = e.target.closest('[data-tier]');
      if (opt) {
        // Switch the dimension WITHOUT closing the modal: flip the tier (and the map
        // behind, for consistency) but keep the same file open — just re-render its
        // breadcrumb in the new representation.
        if (opt.dataset.tier !== window.viewTier?.(level)) window.switchTier?.(opt.dataset.tier, level);
        const m = window._modalNode;
        const node = m && ((typeof activeGraph === 'function' ? activeGraph(m.level).nodes : []).find(n => n.id === m.id)
                           ?? window.DIFF?.[m.level]?.nodes?.find(n => n.id === m.id));
        if (node) document.getElementById('node-modal-hdr-title').innerHTML = window.nodeHeaderHtml(node, m.level);
        document.querySelectorAll('.tier-menu:not([hidden])').forEach(mn => mn.setAttribute('hidden', ''));
        return;
      }
      const root = e.target.closest('[data-mc-root]');
      const btn  = e.target.closest('[data-mc-key]');
      if (!root && !btn) return;
      closeModalSilent();
      if (root) window.drillOutOfGroup?.(level);
      else      window.drillIntoGroup?.(btn.dataset.mcKey, level, Number(btn.dataset.mcDig) || 0);
    });
    document.addEventListener('keydown', e => {
      if (overlay.style.display === 'none') return;   // only while the modal is open
      if (e.key === 'Escape') { closeModal(); return; }
      // Space toggles the modal's selection checkbox (its `change` handler keeps
      // the table row, the SVG node and the selection set in sync). preventDefault
      // both stops the page from scrolling and avoids a double-toggle when the
      // checkbox itself happens to be focused.
      if (e.key === ' ' || e.code === 'Space') {
        const cb = document.getElementById('node-modal-cb');
        if (cb) {
          e.preventDefault();
          cb.checked = !cb.checked;
          cb.dispatchEvent(new Event('change'));
        }
      }
    });
    document.getElementById('node-modal-diagram').addEventListener('click', e => {
      const g = e.target.closest('[data-diag-node]');
      if (!g) return;
      const nodeId = g.dataset.diagNode;
      const level  = document.querySelector('.view.active')?.dataset.view;
      if (!nodeId || !level) return;

      // External cards are inert under modifiers: not selectable and no source to
      // open — only a plain click navigates into them.
      const isExt = g.classList.contains('diag-ext');
      const node  = window.activeGraph?.(level)?.nodes?.find(n => n.id === nodeId);

      // The map's modifier gestures work here too (mirrors setupTooltips):
      //   ⌘/Ctrl → open source, Shift → toggle selection — both skip navigation.
      if (!isExt && node && window.isOpenSrcClick?.(e)) {
        // Anchor at the edge's `#L<line>` for a fan-in neighbour (the `use` site
        // lives in this node's file); fan-out cards resolve to no line.
        const centralId = window._modalNode?.id;
        const line = centralId ? window.connSourceLine?.(nodeId, centralId, level) : null;
        const url = window.nodeSourceUrl?.(node, level, line);
        if (url) window.open(url, '_blank', 'noopener');
        return;
      }
      if (!isExt && node && e.shiftKey) {
        const section = document.querySelector('.view.active');
        // toggleNodeSelected → markPopupSelected updates *every* card for this
        // node (both fan-in and fan-out instances when it's in a cycle).
        window.toggleNodeSelected?.(node, level, section);
        return;
      }
      if (isExt && (e.shiftKey || window.isOpenSrcClick?.(e))) return;  // inert

      if (window.openModalForNode?.(nodeId, level)) window.navPush?.(level, nodeId);
    });
  }
  return overlay;
}

// Set the popup's neighbourhood-diagram SVG and (re)attach the shortcut legend
// *inside* the diagram area, so the legend sits at the bottom-left of the SVG
// rather than the page. Every site that renders the diagram goes through here.
function setModalDiagram(html) {
  const d = document.getElementById('node-modal-diagram');
  if (!d) return;
  // The diagram scrolls inside an inner wrapper; the shortcut legend is a sibling
  // of that wrapper (absolutely placed in the diagram panel), so it stays put in
  // the SVG area instead of scrolling away with the nodes.
  d.innerHTML = `<div class="nm-diagram-scroll">${html}</div>`;
  const hints = document.createElement('div');
  hints.className = 'kbd-hints';
  hints.id = 'node-modal-hints';
  hints.innerHTML = window.kbdHintsHtml?.() ?? '';
  d.appendChild(hints);
  centerDiagramNode(d);
}

// Scroll the diagram so the central (main) node sits at the vertical centre of
// the viewport. The width-fit SVG may be taller than the panel; this anchors the
// view on the node, with the fan-in tier above and fan-out below reachable by
// scroll. Runs after layout (next frame) so the rendered height is known.
function centerDiagramNode(d) {
  requestAnimationFrame(() => {
    const sc = d.querySelector('.nm-diagram-scroll') || d;   // the scrolling wrapper
    const svg = sc.querySelector('svg');
    const frac = svg && parseFloat(svg.dataset.nodeCy || '');
    if (!svg || !isFinite(frac)) return;
    const svgRect  = svg.getBoundingClientRect();
    const contRect = sc.getBoundingClientRect();
    const nodeInContent = (svgRect.top - contRect.top) + sc.scrollTop + frac * svgRect.height;
    sc.scrollTop = Math.max(0, nodeInContent - sc.clientHeight / 2);
  });
}
window.setModalDiagram = setModalDiagram;

function closeModal() {
  window.hideMetricTooltip?.();
  window._modalNode = null;
  window.flyoutHeader?.unmount('modal');
  const overlay = document.getElementById('node-modal-overlay');
  if (overlay) overlay.style.display = 'none';
  document.body.style.overflow = '';
  window.navPush?.(document.querySelector('.view.active')?.dataset.view ?? null, null);
}
function closeModalSilent() {
  window.hideMetricTooltip?.();
  window.flyoutHeader?.unmount('modal');
  const overlay = document.getElementById('node-modal-overlay');
  if (overlay) overlay.style.display = 'none';
  document.body.style.overflow = '';
}
