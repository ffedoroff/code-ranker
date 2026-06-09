// Module a — imports b (b imports a → an a ⇄ b cycle) and exercises TS-specific
// import forms, alias resolution, and the dynamic-import blind spot.

// Named import without extension — DETECTED (a.ts → b.ts; the resolver tries
// .ts/.tsx/.js/.jsx and index.*).
import { beta } from "./b";
// Value import from types.ts — DETECTED (a.ts → types.ts).
import { makeId } from "./types";
// Type-only import — DETECTED (a.ts → types.ts, deduped with the line above).
import type { Id } from "./types";
// `@/` path alias → mapped to the source root — DETECTED (a.ts → util.ts).
import { offset } from "@/util";
// `~utils/*` is a tsconfig path alias the analyzer does NOT understand (only
// `@/` is supported). It is MISCLASSIFIED as a bare external package → a bogus
// External node `~utils` appears instead of an edge to util.ts.
import { spare } from "~utils/util";
// External package — DETECTED → External node `axios`.
import axios from "axios";
// Scoped external package — DETECTED → External node `@scope/util`.
import { thing } from "@scope/util";

export function alpha(): number {
  const _id: Id = makeId();
  return beta() + offset() + spare() + (axios ? 0 : 0) + (thing ? 0 : 0);
}

export async function lazy(): Promise<number> {
  // Dynamic import() — NOT detected; lazy.ts gets no incoming edge.
  const m = await import("./lazy");
  return m.value;
}
