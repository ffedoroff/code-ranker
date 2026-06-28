// `m1 <-> m2` — an isolated 2-node `uses` cycle so the golden covers the
// `mutual` cycle kind alongside the 3-node `chain` SCC formed by a/b/c.
import { two } from "./m2.js";
export function one() {
  return two();
}
