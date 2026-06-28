# Fan-out — Efferent Coupling

**TL;DR**: Fan-out (efferent coupling) counts how many other modules this one
depends on. High fan-out makes a module fragile — it breaks when any of its
many dependencies change — and hard to test or reuse in isolation. Reduce it
by depending on fewer, more abstract collaborators.

## What it measures

`fan_out` is the number of distinct modules this one depends on — its outgoing
flow edges (import paths, qualified references). External-library
dependencies are tracked separately (`fan_out_external`) and not counted here.
Fan-out is the mirror of [fan-in](Fan-in.md).

## Why it matters

A high-fan-out module is coupled to many moving parts:

- **Fragile**: it is exposed to change in every dependency. The more it
  imports, the more often something underneath shifts beneath it.
- **Hard to test**: each dependency must be constructed, mocked, or stubbed to
  test the module in isolation; high fan-out means a heavy test harness.
- **Hard to reuse**: you cannot lift the module into another context without
  dragging its whole dependency cone along.
- **Hard to understand**: following what a module does means following all the
  things it calls.

## What high fan-out looks like

High fan-out typically appears in orchestration code:

- An application entry point / service-wiring module that touches every
  subsystem.
- A "manager" or "coordinator" that pulls in many concrete collaborators.
- A handler that reaches directly into persistence, validation, formatting,
  and external clients all at once.

Some fan-out is inherent at composition roots — that is where wiring lives.
The concern is fan-out in modules that are supposed to hold focused logic.

## Reducing it

For each high-fan-out module:

- **Depend on abstractions**: replace several concrete collaborators with an
  interface the module owns, and inject implementations
  (see [DIP](DIP.md)). The module then depends on one
  abstraction instead of N concretes.
- **Collapse fine-grained dependencies**: if it talks to several small modules
  that always travel together, hide them behind one focused interface.
- **Move misplaced logic**: code that drags in unrelated imports usually
  belongs in a module closer to those dependencies
  (see [LoD](LoD.md) — talk to immediate collaborators, not the
  whole graph).

## How code-ranker surfaces it

`fan_out` is a first-class node metric, a sort option, and the `FANOUT` principle
in the Prompt Generator. The principle ranks modules by fan-out worst-first and
pre-selects **outgoing** connections, so the prompt shows exactly what each
module pulls in.

## Related principles

- [DIP](DIP.md) — depend on abstractions to cut fan-out.
- [LoD](LoD.md) — limit who a module talks to directly.
- [Fan-in](Fan-in.md) — the incoming-dependency mirror.

## References

1. Martin, R. C. "OO Design Quality Metrics: An Analysis of Dependencies"
   (afferent / efferent coupling). 1994.
