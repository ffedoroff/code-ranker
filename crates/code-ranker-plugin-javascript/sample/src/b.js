// Module b — imports a (completing the a ⇄ b cycle) and re-exports from c.

// Named import — DETECTED (b.js → a.js).
import { alpha } from "./a.js";
// Re-export from another file — DETECTED (b.js → c.js) as a `Uses` edge.
export { gamma } from "./c.js";

export function beta() {
  return 2;
}

export function callAlpha() {
  return alpha();
}
