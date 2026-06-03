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

  if (!card.dataset.copy) return;
  navigator.clipboard.writeText(card.dataset.copy).then(() => {
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
        const url = window.nodeSourceUrl?.(node, level);
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
  d.innerHTML = html;
  const hints = document.createElement('div');
  hints.className = 'kbd-hints';
  hints.id = 'node-modal-hints';
  hints.innerHTML = window.kbdHintsHtml?.() ?? '';
  d.appendChild(hints);
}
window.setModalDiagram = setModalDiagram;

function closeModal() {
  window.hideMetricTooltip?.();
  const overlay = document.getElementById('node-modal-overlay');
  if (overlay) overlay.style.display = 'none';
  document.body.style.overflow = '';
  window.navPush?.(document.querySelector('.view.active')?.dataset.view ?? null, null);
}
function closeModalSilent() {
  window.hideMetricTooltip?.();
  const overlay = document.getElementById('node-modal-overlay');
  if (overlay) overlay.style.display = 'none';
  document.body.style.overflow = '';
}
