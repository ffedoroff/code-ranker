const STATUSES = ['added', 'removed', 'affected', 'unchanged'];

const PRESETS = {
  before: { added: false, removed: true,  affected: true, unchanged: true  },
  after:  { added: true,  removed: false, affected: true, unchanged: true  },
  diff:   { added: true,  removed: true,  affected: true, unchanged: true  },
};

const state = {
  graph: 'files',
  show:  { ...PRESETS.diff },
};

// cached dagre graph per graph type; null forces a fresh layout run
let dagreGraph = null;
