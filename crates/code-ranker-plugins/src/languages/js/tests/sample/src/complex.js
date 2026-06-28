// Module `complex` — exercises the per-function complexity metrics
// rust-code-analysis computes for JavaScript (cyclomatic, cognitive, args,
// closures) with real values so the golden guards them. `exits` is NOT emitted
// for JS by the analyzer, so the early `return`s below stay uncounted — an
// analyzer scope limit, not a fixture gap.
export function classify(a, b, c) {
  if (a > 0) {
    if (b > 0) {
      return a + b; // nested if -> cognitive
    }
  } else if (a < 0 || c === 0) {
    return c;
  }
  const scale = (x) => x * 2; // arrow closure -> closures; a/b/c/x -> args
  return scale(a) + b - c;
}
