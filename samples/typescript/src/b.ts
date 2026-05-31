// Module b — imports a (completing the a ⇄ b cycle) and re-exports all of types.

// Named import — DETECTED (b.ts → a.ts).
import { alpha } from "./a";
// `export * from` re-export — DETECTED (b.ts → types.ts).
export * from "./types";

export function beta(): number {
  return 2;
}

export function callAlpha(): number {
  return alpha();
}
