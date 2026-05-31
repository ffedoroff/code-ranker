// Referenced ONLY via `await import("./lazy")` in a.ts. Dynamic import is not
// analyzed, so this file has no incoming edge — an orphan node, demonstrating
// the blind spot.
export const value = 7;
