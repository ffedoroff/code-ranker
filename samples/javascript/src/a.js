// Module a — imports b (b imports a → an a ⇄ b cycle), shows several ESM import
// forms plus the dynamic-import blind spot.

// Named import — DETECTED (a.js → b.js).
import { beta } from "./b.js";
// Namespace import — DETECTED (a.js → c.js).
import * as c from "./c.js";
// Default + external npm package — DETECTED → External node `lodash`.
import _ from "lodash";
// Side-effect import (no bindings) of an external — DETECTED → External `chalk`.
import "chalk";

export function alpha() {
  return _.add(beta(), c.gamma());
}

export async function lazy() {
  // Dynamic import() — a call_expression, NOT an import_statement. NOT detected:
  // no edge to ./dynamic.js is produced, so dynamic.js looks orphaned.
  const m = await import("./dynamic.js");
  return m.payload;
}
