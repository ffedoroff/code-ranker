// Module `complex` — exercises the per-function complexity metrics
// rust-code-analysis computes for TypeScript (cyclomatic, cognitive, exits,
// args) with real values so the golden guards them. (`closures` is covered
// separately by a.test.ts; the analyzer does not count this file's arrow.)
export function classify(a: number, b: number, c: number): number {
  if (a > 0) {
    if (b > 0) {
      return a + b; // nested if -> cognitive; return -> exits
    }
  } else if (a < 0 || c === 0) {
    return c;
  }
  const scale = (x: number): number => x * 2;
  return scale(a) + b - c;
}
