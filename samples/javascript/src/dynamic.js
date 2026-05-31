// This file is referenced ONLY through `await import("./dynamic.js")` in a.js.
// Because dynamic import() is not analyzed, NO edge points here — it is an
// orphan node in the graph, which is exactly the blind spot being demonstrated.
export const payload = 42;
