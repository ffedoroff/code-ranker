# LSP — Liskov Substitution Principle

**TL;DR**: A subtype should be usable everywhere its supertype (an
interface or base type) is expected, without surprises. LSP shows up
as: any type that implements an interface must honour the interface's
contract — return-value invariants, error contracts, failure
conditions, and resource handling — not just the method signatures.
Violations cause runtime astonishment, not compile errors.

## Canonical sources

- Barbara Liskov, "Data Abstraction and Hierarchy" (1988 SIGPLAN
  keynote / 1994 with Jeannette Wing, "A Behavioral Notion of
  Subtyping"):
  <https://dl.acm.org/doi/10.1145/197320.197383>
- Robert C. Martin, "The Liskov Substitution Principle" (1996):
  <https://www.labri.fr/perso/clement/enseignements/ao/LSP.pdf>
- Martin, *Clean Architecture*, Ch. 9.

## The principle

In Liskov's words: if `S` is a subtype of `T`, then objects of type
`T` may be replaced with objects of type `S` without altering any of
the desirable properties of the program.

The crucial word is **desirable** — Liskov is not asking that the
substitute be *identical*, only that it respect the **behavioural
contract** that consumers depend on. Two implementations of an
iterator interface may use entirely different data, but both must:

- Signal end-of-iteration exactly once, and then keep signalling it
  (never produce another element afterwards).
- Not fail except in documented circumstances.
- Report any advertised size estimate consistently with the actual
  number of elements produced.

Each of these is part of the iterator interface's contract, even
though none are type-checked. An implementation that violates them is
**technically valid code** but semantically a Liskov violation: it
compiles but breaks consumers that relied on the contract.

LSP is enforced by **discipline and documentation**, not by the
compiler. The compiler proves *types match*; LSP demands that
*behaviours match*.

## Why it matters

A static type system already prevents many of the failures LSP guards
against in classical OO (when nulls, surprise exceptions, and
variance pitfalls are excluded). What it cannot prevent:

- A textual-formatting implementation that returns wildly different
  format strings across instances, breaking log parsing.
- An iterator implementation that signals end-of-iteration and then
  yields another element later (often legal but undocumented; many
  adapters assume iteration stays finished once it ends).
- A cheap-looking accessor that secretly allocates on every call.
- A destructor/cleanup hook that performs expensive I/O — making
  every container of that type slow to tear down.

Each of these compiles, ships, and gradually erodes the assumption
that "any implementation is interchangeable". Consumers special-case
around the misbehaving implementation, the interface stops being a
clean abstraction, and removing the special case becomes a breaking
change.

## Applying the principle

LSP translates to **interface contracts**. Every interface you define
has, implicitly or explicitly, a behavioural contract that
implementors must honour. Well-designed standard libraries are
unusually explicit about this — for example, the documentation of a
hashing interface typically states:

> Implementations of the hash operation should not produce different
> hashes for values that compare as equal.

That sentence is a Liskov-style contract. The compiler does not
enforce it; the correctness of any hash-based collection assumes it.

The practical rules:

1. **Document every contract requirement** in the interface's
   documentation.
2. **Provide a default implementation** that demonstrates the
   intended behaviour when feasible.
3. **Test implementations against the contract**, not just their own
   happy paths. Provide a contract-test harness consumers can call on
   their own implementations.
4. **Use marker types or capability flags** to surface contract
   assumptions in the type system where the language allows it.

## Violations and remedies

### Anti-pattern: interface without behavioural contract

```
interface Storage {
    put(key, value) -> Result
    get(key) -> Result<Optional<value>>
}
```

What is the contract? Several things are *not* specified:

- Is `get` after `put` guaranteed to return what was put?
  (linearizability? eventual consistency?)
- Is `put` durable? (returns before the write is flushed? after?)
- Are keys case-sensitive?
- What is the maximum value size?
- What concurrency guarantees does a shared instance provide?

A `MemoryStorage` and an `S3Storage` will both "implement" this
interface, but they are not Liskov-substitutable. A test passing on
`MemoryStorage` may fail intermittently on `S3Storage`.

### Idiomatic fix: contract in the docs + contract tests

```
// Key-value store.
//
// Contract:
//
// 1. put(k, v) followed by get(k) on the same instance MUST return
//    Some(v). Implementations targeting eventually consistent
//    backends MUST block in put until the value is visible to
//    subsequent get calls.
// 2. Keys are case-sensitive.
// 3. Both methods MAY return an error only for I/O failures, not for
//    missing keys (which return None).
// 4. Concurrent calls on the same instance are safe. There is no
//    ordering guarantee between concurrent writers.
interface Storage {
    put(key, value) -> Result
    get(key) -> Result<Optional<value>>
}

// Contract-conformance test harness. Expose it so downstream
// implementations can call it.
function assert_contract(s: Storage) {
    s.put("k", "v")
    assert s.get("k") == Some("v")
    assert s.get("K") == None  // keys must be case-sensitive
    // ... etc
}
```

Every downstream `Storage` implementation gets a one-line conformance
test: `assert_contract(new MyStorage())`.

### Anti-pattern: iterator that violates the "stays finished" contract

```
class MyStream implements Iterator {
    next() -> Optional<Event> {
        if this.reconnect_pending {
            return Some(this.next_after_reconnect())  // surprise: a value after end
        }
        return this.queue.pop()
    }
}
```

The iterator interface *may* yield a value after signalling end (in
some languages this is legal), but most adapters assume "once iteration
ends, it stays ended". A consumer that wraps `MyStream` in a
look-ahead adapter will silently miss events after the first
reconnect.

### Idiomatic fix: explicit type signalling, or finalize internally

If the stream really can resume, do not implement the plain iterator
interface. Use a dedicated stream abstraction whose contract permits
resumption. If it cannot resume, **signal end only at the true end and
mark the type as a "fused" iterator** (one that promises it stays
ended):

```
class MyStream implements FusedIterator {}
```

A "fused" marker promises the iterator stays ended permanently after
the first end signal. Adapters use this marker to skip defensive
checks.

### Anti-pattern: method that may fail without saying so

```
interface Cache {
    get(k) -> Bytes  // crashes if key missing
}
```

Consumers writing `if cache.get(k).is_empty() { ... }` will crash on
the first miss. The signature lies.

### Idiomatic fix: encode partial functions in the type

```
interface Cache {
    get(k) -> Optional<Bytes>
}
```

If failing hard is the right behaviour (an internal invariant
violation, not a user error), document it as a failure condition.
Better still, return a result with an error variant that names the
invariant.

### Anti-pattern: violating hash/equality consistency

```
type User { id: UserId, last_seen: Timestamp }

// equality compares only id
equals(a: User, b: User) -> bool { a.id == b.id }

// but hashing mixes in last_seen
hash(u: User) -> int { combine(u.id, u.last_seen) }  // not in equals!
```

Two `User` values with the same `id` and different `last_seen` are
equal but have different hashes. A hash-based lookup will sometimes
miss them. This is an LSP violation against the documented contract:
equal values must hash equally.

### Idiomatic fix: derive both from the same fields

```
type User { id: UserId, last_seen: Timestamp }

equals(a, b) -> bool { a.id == b.id }
hash(u)      -> int  { hash(u.id) }   // same fields as equality
```

If `last_seen` should not affect equality, keep it out of both
equality and hashing. Generate or define them together so they stay
in sync mechanically.

### Anti-pattern: cleanup hook that does I/O without an explicit "close"

```
class FileHandle {
    on_destroy() {
        this.fsync()                  // may fail hard during teardown
        remove_file(this.path)
    }
}
```

A cleanup/destructor hook is called from arbitrary contexts, including
error unwinding. Failing hard there can abort the whole program.
Consumers have no way to handle the error.

### Idiomatic fix: explicit `close()` returning a result, best-effort cleanup

```
class FileHandle {
    close() -> Result {
        this.fsync()?
        remove_file(this.path)?
        return Ok
    }

    on_destroy() {
        // best-effort cleanup; never fail hard
        try { this.fsync() }
        try { remove_file(this.path) }
    }
}
```

Consumers can choose to call `close()` for error visibility; the
cleanup hook guarantees no hard failure for clean-up that simply did
not happen because of an earlier error path.

## LSP across module boundaries

When a third-party module depends on your interface, you ship its
behavioural contract too. Versioned contract changes are breaking even
when types match. If you tighten or loosen the contract of
`Storage.get`, downstream implementations that were previously
conformant may become non-conformant.

The mitigation: state the contract in the interface documentation,
version it ("contract version 1.0"), and treat contract changes as
versioning events even when no signature changes.

## How code-ranker detects LSP violations

LSP violations are usually invisible to a graph analyzer — they live
in implementation bodies and runtime behaviour. But code-ranker can
flag *structural risk*:

| Signal | LSP interpretation |
|---|---|
| Interface with N implementations and short documentation (no contract section) | Implementors have no shared contract; each implementation will diverge. (Detection requires parsing doc comments — future rule.) |
| Multiple iterator implementations lacking a "stays finished" guarantee on types whose iteration could resume after ending | Documented LSP-tier risk for iterators. Out of scope for syntactic analysis. |
| A cleanup/destructor hook containing a call to functions known to fail hard | Teardown-failure risk; out of static scope but interesting target for a future syntactic linter. |
| A type whose hashing and equality use non-overlapping field sets | Direct hash/equality consistency check. Requires AST-level field analysis. |

The honest answer is that LSP is mostly a documentation discipline —
code-ranker's main contribution is to *flag interfaces that have no
contract section* and to *recommend writing one*, not to verify
behaviour.

## Suggested recommendation template

> **LSP risk**: interface `Storage` has 6 implementations across the
> project and no contract section in its documentation. Without a
> stated behavioural contract, implementations diverge silently and
> consumers special-case around them. Add a contract section
> documenting required invariants for every method, then export a
> contract-test helper (`assert_contract(t: Storage)`) that downstream
> implementations can call from their tests.

## Related principles

- [SRP](SRP.md) — narrow interfaces are easier
  to write contracts for than broad ones.
- [ISP](ISP.md) — clients depend on small
  contracts, not large ones; LSP gets easier with each split.
- [Make Invalid States Unrepresentable](MISU.md)
  — encode contract requirements in types where possible (e.g. a
  non-empty collection type instead of a "must not be empty" note in
  the docs).

## References

1. Liskov, B. and Wing, J. "A Behavioral Notion of Subtyping". ACM
   TOPLAS 16(6), 1994.
   <https://dl.acm.org/doi/10.1145/197320.197383>
2. Martin, R. C. "The Liskov Substitution Principle". 1996.
   <https://www.labri.fr/perso/clement/enseignements/ao/LSP.pdf>
3. Martin, R. C. *Clean Architecture*. Ch. 9.
