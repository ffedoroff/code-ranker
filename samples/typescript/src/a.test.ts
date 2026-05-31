// Test file (under src/ so the plugin collects it). Its import is DETECTED
// like any other (a.test.ts → a.ts).
import { alpha } from "./a";

test("alpha is positive", () => {
  expect(alpha()).toBeGreaterThanOrEqual(0);
});
