// Test file (kept under src/ so the plugin collects it). Its import is DETECTED
// like any other (a.test.js → a.js).
import { alpha } from "./a.js";

test("alpha is numeric", () => {
  expect(typeof alpha()).toBe("number");
});
