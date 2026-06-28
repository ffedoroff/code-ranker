// chain3 -> chain1 — closes the three-node cycle.
import { one } from "./chain1";
export function three(): number {
  return one();
}
