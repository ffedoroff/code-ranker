// Online principle docs (principles/<lang>/<slug>.md on GitHub) keyed by preset.
// Only the 12 named principles map; the meta-presets (fix cycles, …) have none.
const PRINCIPLE_DOCS = {
  ADP:   'acyclic-dependencies-principle',
  SRP:   'solid-single-responsibility',
  OCP:   'solid-open-closed',
  LSP:   'solid-liskov-substitution',
  ISP:   'solid-interface-segregation',
  DIP:   'solid-dependency-inversion',
  DRY:   'dry',
  KISS:  'kiss',
  LoD:   'law-of-demeter',
  MISU:  'make-invalid-states-unrepresentable',
  CoI:   'composition-over-inheritance',
  YAGNI: 'yagni',
};
const PRINCIPLES_URL = 'https://github.com/ffedoroff/code-split/blob/main/principles';

// The principle-corpus language for the analyzed plugin (JS/TS share `typescript`).
function principleLang() {
  const plugin = (window.AFTER ?? window.BEFORE)?.plugin || 'rust';
  return plugin === 'javascript' ? 'typescript' : plugin;
}

// Online URL of a preset's full principle doc, or null for the meta-presets.
function principleUrl(key) {
  const slug = PRINCIPLE_DOCS[key];
  return slug ? `${PRINCIPLES_URL}/${principleLang()}/${slug}.md` : null;
}

function openExportPopup(level) {
  const selectedIds = window._ntSelected?.[level];
  const allNodes    = window.DIFF?.[level]?.nodes || [];
  const allEdges    = window.DIFF?.[level]?.edges || [];
  const selNodes    = allNodes.filter(n => selectedIds?.has(n.id));

  const cleanPath = p => (p || '').replace(/^\{[^}]+\}\//, '');

  const PROMPTS = {
    'fix cycles':
`Research and propose concrete ways to break the dependency cycles in the selected modules. Consider:
- Extracting shared traits/interfaces into a new module
- Applying Dependency Inversion (high-level modules depend on abstractions)
- Introducing an event/message-passing layer
- Reorganising module boundaries so the dependency graph becomes a DAG`,

    'Reduce Complexity':
`Research approaches to reduce complexity in the selected modules. Consider:
- Splitting large modules into smaller, single-responsibility units
- Extracting repeated patterns into shared utilities
- Simplifying deeply nested control flow
- Breaking up large functions into focused helpers`,

    'Split Components':
`Research a splitting strategy for the selected modules that:
- Preserves existing API contracts
- Minimises coupling in the new structure
- Follows the Single Responsibility Principle
- Does not introduce new dependency cycles`,

    'ADP':
`ADP — Acyclic Dependencies Principle

The dependency graph between modules must form a DAG. When module A depends
on module B, no chain of dependencies should bring B back to A.

Identify any cycles in the selected modules. For each cycle, propose a concrete
refactoring (extract a shared abstraction, invert a dependency, split a module)
that makes the graph acyclic without breaking existing functionality.`,

    'SRP':
`SRP — Single Responsibility Principle

A module should have one reason to change — it should serve one actor
and encapsulate one coherent set of decisions.

For each selected module, identify whether it has more than one responsibility.
Propose how to split responsibilities so each module changes for only one reason,
and specify the new module boundaries.`,

    'OCP':
`OCP — Open/Closed Principle

A module should be open for extension but closed for modification.
In Rust: prefer adding a new impl or trait over editing existing match arms
or branching logic; use \`#[non_exhaustive]\` and sealed traits.

For each selected module, identify extension points that currently require editing
existing code. Propose how to introduce trait-based or enum-based extension so
new behaviour can be added without modifying these modules.`,

    'LSP':
`LSP — Liskov Substitution Principle

Any impl Trait for Foo must honour the trait's full contract —
return-value invariants, error conditions, panic behaviour, and
resource ownership — not just the method signatures.

Identify trait implementations in the selected modules. For each, check whether
the implementation can be substituted for any other impl of the same trait without
surprising callers. Flag violations and propose fixes.`,

    'ISP':
`ISP — Interface Segregation Principle

Clients should not be forced to depend on methods they do not use.
Prefer many small, focused traits over one wide trait.

Identify traits in the selected modules that are wider than their consumers require.
Propose how to split them into narrower traits so each consumer only depends on
what it actually uses.`,

    'DIP':
`DIP — Dependency Inversion Principle

High-level modules should not depend on low-level modules; both should depend
on abstractions. In Rust: domain crates define traits; infra crates implement
them; the app wires concrete types at the composition root.

Identify places in the selected modules where a high-level module imports a
concrete low-level type. Propose an abstraction (trait) to invert each such
dependency, and specify where the concrete wiring should live.`,

    'DRY':
`DRY — Don't Repeat Yourself

Every piece of knowledge must have a single authoritative representation.
DRY is about knowledge duplication, not just code duplication.

Identify concepts, rules, or policies that are duplicated across the selected
modules. For each duplication, propose a canonical location and the refactoring
needed to consolidate it.`,

    'KISS':
`KISS — Keep It Simple

When two designs solve the same problem, prefer the simpler one.
In Rust: fewer generics, fewer indirection layers, \`enum + match\`
before \`Box<dyn Trait>\`, a function before a trait.

Identify over-engineered abstractions in the selected modules. For each, explain
the simpler alternative and estimate the risk of the simplification.`,

    'LoD':
`Law of Demeter — Principle of Least Knowledge

A method should only call methods on: itself, its direct fields,
its parameters, and objects it constructs locally.
Avoid \`x.foo().bar().baz()\` chains that traverse object graphs.

Identify method chains or deep field traversals in the selected modules that
violate LoD. For each, propose a narrow accessor or a facade that exposes only
what the caller needs, reducing coupling.`,

    'MISU':
`Make Invalid States Unrepresentable

Move correctness from runtime checks into the type system.
Use Rust enums, lifetimes, and typestate so that invalid states
cause compile errors rather than runtime panics.

Identify data structures or function signatures in the selected modules where
invalid states are possible at runtime. For each, propose a type-level encoding
(enum variant, newtype, typestate) that makes the invalid state unreachable by
construction.`,

    'CoI':
`Composition Over Inheritance

Build behaviour by composing small, focused traits and structs.
Rust has no class inheritance — the question is how to compose:
trait bounds, blanket impls, delegation, newtype.

Identify any large traits or structs in the selected modules that accumulate
behaviour. Propose how to decompose them into smaller composable pieces and show
how consumers would assemble the behaviour they need.`,

    'YAGNI':
`YAGNI — You Aren't Gonna Need It

Build for the problem you have now. Don't add a trait for a hypothetical
second implementation, a generic for a hypothetical second type, or a
\`pub\` API for an internal use case.

Identify abstractions, generics, or public APIs in the selected modules that were
added speculatively. For each, assess whether it is actually used by multiple
distinct callers today, and propose simplification if not.`,
  };

  // ── popup DOM (created once) ──────────────────────────────────────────
  let overlay = document.getElementById('export-popup-overlay');
  if (!overlay) {
    const principleKeys = ['ADP','SRP','OCP','LSP','ISP','DIP','DRY','KISS','LoD','MISU','CoI','YAGNI'];
    const principleLabels = {
      'ADP':  'ADP (fix cycles)',
      'SRP':  'SRP (split responsibilities)',
      'OCP':  "OCP (extend, don't edit)",
      'LSP':  'LSP (honest impls)',
      'ISP':  'ISP (narrow interfaces)',
      'DIP':  'DIP (depend on abstractions)',
      'DRY':  'DRY (no duplication)',
      'KISS': 'KISS (simplify)',
      'LoD':  'LoD (no chaining)',
      'MISU': 'MISU (encode in types)',
      'CoI':  "CoI (compose, don't inherit)",
      'YAGNI':'YAGNI (cut speculation)',
    };
    overlay = document.createElement('div');
    overlay.id = 'export-popup-overlay';
    overlay.innerHTML =
      '<div id="export-popup">' +
        '<div id="export-popup-hdr">' +
          '<h3>Prompt Generator</h3>' +
          '<button id="export-popup-close">✕</button>' +
        '</div>' +
        '<div class="exp-modes">' +
          '<div class="exp-cb-group">' +
            '<label class="exp-mode-cb"><input type="checkbox" data-mode="ids" checked> IDs</label>' +
            '<label class="exp-mode-cb"><input type="checkbox" data-mode="paths"> Paths</label>' +
            '<label class="exp-mode-cb"><input type="checkbox" data-mode="conn-common"> connections common</label>' +
            '<label class="exp-mode-cb"><input type="checkbox" data-mode="conn-in"> connections in</label>' +
            '<label class="exp-mode-cb"><input type="checkbox" data-mode="conn-out"> connections out</label>' +
          '</div>' +
          '<div class="exp-source-or">OR</div>' +
          '<div class="exp-source-group">' +
            '<label class="exp-src-radio"><input type="radio" name="exp-source" value="selected" checked> Selected</label>' +
            '<label class="exp-src-radio"><input type="radio" name="exp-source" value="recommended"> Recommended</label>' +
            '<input type="number" class="exp-rec-count" min="1" max="999" value="5">' +
          '</div>' +
        '</div>' +
        '<div class="exp-textarea-wrap">' +
          '<textarea id="export-textarea" readonly></textarea>' +
          '<button class="exp-copy-btn">Copy <span class="exp-copy-icon">⎘</span></button>' +
        '</div>' +
        '<div class="exp-presets">' +
          '<div class="exp-presets-label">Presets</div>' +
          '<div class="exp-preset-btns">' +
            '<button class="exp-preset-btn" data-preset="Reduce Complexity">Reduce Complexity</button>' +
            '<button class="exp-preset-btn" data-preset="Split Components">Split Components</button>' +
            principleKeys.map(k => `<button class="exp-preset-btn" data-preset="${k}">${principleLabels[k]}</button>`).join('') +
          '</div>' +
        '</div>' +
      '</div>';
    document.body.appendChild(overlay);

    const closeExport = () => { overlay.style.display = 'none'; document.body.style.overflow = ''; };
    document.getElementById('export-popup-close').addEventListener('click', closeExport);
    overlay.addEventListener('mousedown', e => { if (e.target === overlay) closeExport(); });
    document.addEventListener('keydown', e => { if (e.key === 'Escape' && overlay.style.display !== 'none') closeExport(); });
    overlay.querySelector('.exp-copy-btn').addEventListener('click', () => {
      const ta = document.getElementById('export-textarea');
      navigator.clipboard?.writeText(ta.value).then(() => {
        const btn = overlay.querySelector('.exp-copy-btn');
        const orig = btn.innerHTML;
        btn.innerHTML = 'Copied ✓';
        setTimeout(() => { btn.innerHTML = orig; }, 1400);
      });
    });
  }

  // Which checkboxes to activate for each preset (ids always on)
  const PRESET_CHECKS = {
    'fix cycles':        ['ids', 'conn-common'],
    'Reduce Complexity': ['ids', 'paths'],
    'Split Components':  ['ids', 'conn-common', 'conn-out'],
    'ADP':  ['ids', 'conn-common'],
    'SRP':  ['ids', 'conn-in', 'conn-out'],
    'OCP':  ['ids', 'paths'],
    'LSP':  ['ids', 'paths'],
    'ISP':  ['ids', 'conn-in'],
    'DIP':  ['ids', 'conn-common', 'conn-out'],
    'DRY':  ['ids', 'paths'],
    'KISS': ['ids', 'paths'],
    'LoD':  ['ids', 'conn-common', 'conn-out'],
    'MISU': ['ids', 'paths'],
    'CoI':  ['ids', 'conn-common'],
    'YAGNI':['ids', 'conn-out'],
  };

  // Rebind handlers each open (closures capture fresh selNodes/edges)
  const ta = document.getElementById('export-textarea');
  let activePresetKey = null;

  const getRecommendedNodes = count => {
    let candidates = allNodes.filter(n => !n.external && n.status !== 'removed');
    if (activePresetKey === 'ADP' || activePresetKey === 'fix cycles') {
      const cy = window.CYCLES?.[level];
      if (cy) {
        const inCycle = id => cy.nodeCycleStatus?.get(id) != null;
        candidates.sort((a, b) => (inCycle(b.id) ? 1 : 0) - (inCycle(a.id) ? 1 : 0) || (b.hk || 0) - (a.hk || 0));
      } else {
        candidates.sort((a, b) => (b.hk || 0) - (a.hk || 0) || (b.loc || 0) - (a.loc || 0));
      }
    } else {
      candidates.sort((a, b) => (b.hk || 0) - (a.hk || 0) || (b.loc || 0) - (a.loc || 0));
    }
    return candidates.slice(0, count);
  };

  const getActiveNodes = () => {
    const src = overlay.querySelector('input[name="exp-source"]:checked')?.value;
    if (src === 'recommended') {
      const count = parseInt(overlay.querySelector('.exp-rec-count')?.value) || 5;
      return getRecommendedNodes(count);
    }
    return selNodes;
  };

  const buildContent = () => {
    const activeNodes = getActiveNodes();
    const activeSet   = new Set(activeNodes.map(n => n.id));
    const innerEdges  = allEdges.filter(e => activeSet.has(e.from) && activeSet.has(e.to));
    const outerEdges  = allEdges.filter(e => activeSet.has(e.from) !== activeSet.has(e.to));

    const cbs  = [...overlay.querySelectorAll('.exp-mode-cb input')];
    const on   = id => cbs.find(c => c.dataset.mode === id)?.checked;
    const parts = [];
    if (activePresetKey) {
      const promptText = PROMPTS[activePresetKey] || '';
      if (promptText) {
        // Append a link to the full principle online so the prompt is self-contained.
        const url = principleUrl(activePresetKey);
        parts.push(url ? `${promptText}\n\nFull principle: ${url}` : promptText);
      }
    }
    if (on('ids')) {
      parts.push('node ids:\n' + activeNodes.map(n => n.id).join('\n'));
    }
    if (on('paths')) {
      const lines = activeNodes.map(n => {
        const p    = cleanPath(n.path) || n.id;
        const line = n.line != null ? `:${n.line}` : '';
        return p + line;
      }).join('\n');
      parts.push('node paths:\n' + lines);
    }
    const edgeFmt = edges => edges.length ? edges.map(e => `${e.from}  →  ${e.to}  [${e.kind}]`).join('\n') : '(none)';
    if (on('conn-common')) parts.push('connections common:\n' + edgeFmt(innerEdges));
    if (on('conn-in'))     parts.push('connections in:\n'     + edgeFmt(outerEdges.filter(e => activeSet.has(e.to))));
    if (on('conn-out'))    parts.push('connections out:\n'    + edgeFmt(outerEdges.filter(e => activeSet.has(e.from))));
    ta.value = parts.join('\n\n');
  };

  overlay.querySelectorAll('.exp-mode-cb input').forEach(cb => { cb.onchange = buildContent; });

  overlay.querySelectorAll('input[name="exp-source"]').forEach(r => { r.onchange = () => {
    const isRec = overlay.querySelector('input[name="exp-source"]:checked')?.value === 'recommended';
    overlay.querySelector('.exp-rec-count').style.display = isRec ? '' : 'none';
    buildContent();
  }; });
  overlay.querySelector('.exp-rec-count').addEventListener('input', buildContent);

  const applyPresetChecks = key => {
    const active = key ? (PRESET_CHECKS[key] || ['ids']) : ['ids'];
    overlay.querySelectorAll('.exp-mode-cb input').forEach(cb => {
      cb.checked = active.includes(cb.dataset.mode);
    });
  };

  overlay.querySelectorAll('.exp-preset-btn').forEach(btn => {
    btn.onclick = () => {
      const key = btn.dataset.preset;
      if (activePresetKey === key) {
        activePresetKey = null;
        btn.classList.remove('exp-preset-btn--active');
        applyPresetChecks(null);
      } else {
        activePresetKey = key;
        overlay.querySelectorAll('.exp-preset-btn').forEach(b => b.classList.remove('exp-preset-btn--active'));
        btn.classList.add('exp-preset-btn--active');
        applyPresetChecks(key);
      }
      buildContent();
    };
  });

  // Reset: only ids checked, no active preset, source = selected
  activePresetKey = null;
  overlay.querySelectorAll('.exp-preset-btn').forEach(b => b.classList.remove('exp-preset-btn--active'));
  overlay.querySelectorAll('.exp-mode-cb input').forEach(c => { c.checked = c.dataset.mode === 'ids'; });
  overlay.querySelectorAll('input[name="exp-source"]').forEach(r => { r.checked = r.value === 'selected'; });
  overlay.querySelector('.exp-rec-count').style.display = 'none';
  buildContent();
  overlay.style.display = 'flex';
  document.body.style.overflow = 'hidden';
}
