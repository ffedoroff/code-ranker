// chain2 -> chain3 (second link).
import { three } from "./chain3";
export function two(): number {
  return three();
}
