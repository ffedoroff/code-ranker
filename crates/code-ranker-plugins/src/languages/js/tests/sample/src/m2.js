// `m2 -> m1` closes the 2-node mutual cycle.
import { one } from "./m1.js";
export function two() {
  return one();
}
