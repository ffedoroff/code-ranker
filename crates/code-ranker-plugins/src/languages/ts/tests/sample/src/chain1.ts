// chain1 -> chain2 (first link of the 3-node chain cycle).
import { two } from "./chain2";
export function one(): number {
  return two();
}
