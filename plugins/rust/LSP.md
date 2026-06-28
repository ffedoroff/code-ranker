# LSP — Liskov Substitution Principle (in Rust)

**TL;DR**: A `T` should be usable everywhere a `dyn Trait` is expected,
without surprises. In Rust, LSP shows up as: any `impl Trait for Foo`
must honour the trait's contract — return-value invariants, error
contracts, panic conditions, and resource ownership — not just the
method signatures. Violations cause runtime astonishment, not compile
errors.

## Canonical sources

- Barbara Liskov, "Data Abstraction and Hierarchy" (1988 SIGPLAN
  keynote / 1994 with Jeannette Wing, "A Behavioral Notion of
  Subtyping"):
  <https://dl.acm.org/doi/10.1145/197320.197383>
- Robert C. Martin, "The Liskov Substitution Principle" (1996):
  <https://www.labri.fr/perso/clement/enseignements/ao/LSP.pdf>
- Martin, *Clean Architecture*, Ch. 9.
- Yoshua Wuyts, "From design to type system" (2021): trait contracts
  in Rust. <https://blog.yoshuawuyts.com/from-design-to-type-system/>
- Rust API Guidelines, "Predictability":
  <https://rust-lang.github.io/api-guidelines/predictability.html>

## The principle

In Liskov's words: if `S` is a subtype of `T`, then objects of type
`T` may be replaced with objects of type `S` without altering any of
the desirable properties of the program.

The crucial word is **desirable** — Liskov is not asking that the
substitute be *identical*, only that it respect the **behavioural
contract** that consumers depend on. Two implementations of `Iterator`
may use entirely different data, but both must:

- Return `None` exactly once at the end of iteration (and then keep
  returning `None`).
- Not panic except in documented circumstances.
- Honour `size_hint` such that the actual count is in
  `[lower, upper.unwrap_or(usize::MAX)]`.

Each of these is part of `Iterator`'s contract, even though none are
type-checked. An implementation that violates them is **technically
valid Rust** but semantically a Liskov violation: it compiles but
breaks consumers that relied on the contract.

LSP is enforced by **discipline and documentation**, not by the
compiler. The compiler proves *types match*; LSP demands that
*behaviours match*.

## Why it matters

Rust's type system already prevents many of the failures LSP guards
against in classical OO (no implicit nulls, no surprise exceptions,
no covariance/contravariance landmines). What it cannot prevent:

- An `impl Display for MyError` that returns wildly different format
  strings across instances, breaking log parsing.
- An `impl Iterator for MyStream` that returns `None` and then `Some`
  later (legal but undocumented; many adapters assume fused iteration).
- An `impl AsRef<str> for MyBuffer` that allocates on every call.
- An `impl Drop for MyHandle` that performs expensive I/O — making
  every container of `MyHandle` slow to drop in destructors.

Each of these compiles, ships, and gradually erodes the assumption
that "any impl is interchangeable". Consumers special-case around the
misbehaving impl, the trait stops being a clean abstraction, and
removing the special case becomes a breaking change.

## In Rust

LSP in Rust translates to **trait contracts**. Every trait you define
has, implicitly or explicitly, a behavioural contract that
implementors must honour. The Rust standard library is unusually
explicit about this — read the rustdoc for `Iterator`, `Hash`, `Ord`,
or `Eq`:

> The `Hash` trait is used to implement hashing in association with
> equality. Implementations of `Hash` should not produce different
> hashes from values that are equal.

That sentence is a Liskov-style contract. The compiler does not
enforce it; `HashMap` and `HashSet` correctness assumes it.

The practical rules:

1. **Document every contract requirement** in the trait's rustdoc.
2. **Provide a default implementation** that demonstrates the
   intended behaviour when feasible.
3. **Test trait implementations against the contract**, not just
   their own happy paths. Provide a `trait_contract_test!` macro or
   a test harness consumers can call on their own impls.
4. **Use marker traits** (`Send`, `Sync`, `FusedIterator`,
   `ExactSizeIterator`) to surface contract assumptions in the type
   system.

## Violations and remedies

### Anti-pattern: trait without behavioural contract

```rust
pub trait Storage {
    fn put(&self, key: &str, value: &[u8]) -> Result<()>;
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
}
```

What is the contract? Several things are *not* specified:

- Is `get` after `put` guaranteed to return what was put?
  (linearizability? eventual consistency?)
- Is `put` durable? (returns before fsync? after?)
- Are keys case-sensitive?
- What is the maximum value size?
- What concurrency guarantees does an `&self` impl provide?

A `MemoryStorage` and a `S3Storage` will both "implement" this trait,
but they are not Liskov-substitutable. A test passing on
`MemoryStorage` may fail intermittently on `S3Storage`.

### Idiomatic fix: contract in rustdoc + contract tests

```rust
/// Key-value store.
///
/// # Contract
///
/// 1. `put(k, v)` followed by `get(k)` (on the same `&self`) MUST
///    return `Ok(Some(v))`. Implementations targeting eventually
///    consistent backends MUST block in `put` until the value is
///    visible to subsequent `get` calls.
/// 2. Keys are case-sensitive.
/// 3. Both methods MAY return an `Err` only for I/O failures, not
///    for missing keys (which return `Ok(None)`).
/// 4. Concurrent calls on the same `&self` are safe. There is no
///    ordering guarantee between concurrent writers.
pub trait Storage: Send + Sync {
    fn put(&self, key: &str, value: &[u8]) -> Result<()>;
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
}

/// Contract-conformance test harness. Re-export so downstream impls
/// can call it.
pub mod testing {
    use super::*;
    pub fn assert_contract<S: Storage>(s: &S) {
        s.put("k", b"v").unwrap();
        assert_eq!(s.get("k").unwrap().as_deref(), Some(&b"v"[..]));
        assert_eq!(s.get("K").unwrap(), None, "keys must be case-sensitive");
        // ... etc
    }
}
```

Every downstream `impl Storage` gets a one-line conformance test:
`storage::testing::assert_contract(&MyStorage::new())`.

### Anti-pattern: `Iterator` that violates the fused contract

```rust
impl Iterator for MyStream {
    type Item = Event;
    fn next(&mut self) -> Option<Event> {
        if self.reconnect_pending {
            return Some(self.next_after_reconnect());  // surprise: Some after None
        }
        self.queue.pop()
    }
}
```

`Iterator::next` *may* return `Some` after `None` (legal), but most
adapters (`Chain`, `Peekable`, `Fuse`) assume "after `None`, always
`None`". A consumer that wraps `MyStream` in `Peekable` will silently
miss events after the first reconnect.

### Idiomatic fix: explicit type signalling, or fuse internally

If the stream really can resume, do not implement `Iterator`. Use a
dedicated `Stream` trait (futures crate) or a custom trait whose
contract permits resumption. If it cannot resume, **return `None`
only at the true end and wrap with `FusedIterator`**:

```rust
impl FusedIterator for MyStream {}
```

`FusedIterator` is a marker trait that promises the iterator returns
`None` permanently after the first `None`. Adapters use this marker
to skip defensive checks.

### Anti-pattern: trait method that may panic without saying so

```rust
pub trait Cache {
    fn get(&self, k: &str) -> Vec<u8>;  // panics if key missing
}
```

Consumers writing `if cache.get(k).is_empty() { ... }` will crash on
the first miss. The signature lies.

### Idiomatic fix: encode partial functions in the type

```rust
pub trait Cache {
    fn get(&self, k: &str) -> Option<Vec<u8>>;
}
```

If panicking is the right behaviour (an internal invariant violation,
not a user error), document it as a panic condition. Better still,
return a `Result` with an error variant that names the invariant.

### Anti-pattern: violating `Hash`-`Eq` consistency

```rust
#[derive(Eq, PartialEq)]
struct User { id: UserId, last_seen: Instant }

impl Hash for User {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.id.hash(h);
        self.last_seen.hash(h);  // not in Eq!
    }
}
```

Two `User` values with the same `id` and different `last_seen` are
`==` but have different hashes. `HashMap<User, V>::get` will sometimes
miss them. This is a LSP violation against `Hash`'s documented
contract: equal values must hash equally.

### Idiomatic fix: derive both, or hash only fields used in equality

```rust
#[derive(Eq, PartialEq, Hash)]
struct User { id: UserId, last_seen: Instant }
```

If `last_seen` should not affect equality, remove it from both `Eq`
and `Hash`. Use `#[derive]` to keep them in sync mechanically.

### Anti-pattern: `Drop` impl that does I/O without an explicit "close"

```rust
impl Drop for FileHandle {
    fn drop(&mut self) {
        self.fsync().expect("fsync failed");  // panic on drop
        std::fs::remove_file(&self.path).ok();
    }
}
```

`Drop` is called from arbitrary contexts, including unwinding. A
panic in `drop()` during an unwind aborts the process. Consumers
have no way to handle the error.

### Idiomatic fix: explicit `close()` returning `Result`, no-op `Drop`

```rust
impl FileHandle {
    pub fn close(self) -> io::Result<()> {
        self.fsync()?;
        std::fs::remove_file(&self.path)?;
        Ok(())
    }
}

impl Drop for FileHandle {
    fn drop(&mut self) {
        // best-effort cleanup; never panic
        let _ = self.fsync();
        let _ = std::fs::remove_file(&self.path);
    }
}
```

Consumers can choose to call `close()` for error visibility; `Drop`
guarantees no panic for clean-up that simply did not happen because
of an earlier error path.

## LSP across crates

When a third-party crate depends on your trait, you ship its
behavioural contract too. Versioned contract changes are breaking
even when types match. If you tighten or loosen the contract of
`Storage::get`, downstream impls that were previously conformant may
become non-conformant.

The mitigation: state the contract in the trait rustdoc, version
it ("contract version 1.0"), and treat contract changes as semver
events even when no signature changes.

## How code-ranker detects LSP violations

LSP violations are usually invisible to a graph analyzer — they live
in implementation bodies and runtime behaviour. But code-ranker can flag
*structural risk*:

| Signal | LSP interpretation |
|---|---|
| Trait with N implementations and short rustdoc (no contract section) | Implementors have no shared contract; each impl will diverge. (Detection requires parsing doc comments — future rule.) |
| Multiple impls of `Iterator` lacking `FusedIterator` impl on types whose `next` could return `Some` after `None` | Documented LSP-tier risk for `Iterator`. Out of scope for syntactic analysis. |
| `impl Drop` containing call-site to functions known to panic | Drop-panic risk; out of static scope but interesting target for a future syntactic linter. |
| `impl Hash` for a type also `impl PartialEq` with non-overlapping field sets | Direct `Hash`/`Eq` consistency check. Requires AST-level field analysis. |

The honest answer is that LSP is mostly a documentation discipline —
code-ranker's main contribution is to *flag traits that have no contract
section* and to *recommend writing one*, not to verify behaviour.

## Suggested recommendation template

> **LSP risk**: trait `Storage` has 6 implementations across the
> workspace and no `# Contract` section in its rustdoc. Without a
> stated behavioural contract, implementations diverge silently and
> consumers special-case around them. Add a `# Contract` section
> documenting required invariants for every method, then export a
> `pub mod testing` with a `assert_contract<T: Storage>(t: &T)`
> helper that downstream impls can call from their tests.
>
> References:
>  - <https://blog.yoshuawuyts.com/from-design-to-type-system/>
>  - <https://rust-lang.github.io/api-guidelines/predictability.html>

<!-- doc:base "Related principles" -->

## References

1. Liskov, B. and Wing, J. "A Behavioral Notion of Subtyping". ACM
   TOPLAS 16(6), 1994.
   <https://dl.acm.org/doi/10.1145/197320.197383>
2. Martin, R. C. "The Liskov Substitution Principle". 1996.
   <https://www.labri.fr/perso/clement/enseignements/ao/LSP.pdf>
3. Martin, R. C. *Clean Architecture*. Ch. 9.
4. Wuyts, Y. "From design to type system". 2021.
   <https://blog.yoshuawuyts.com/from-design-to-type-system/>
5. Rust API Guidelines, "Predictability".
   <https://rust-lang.github.io/api-guidelines/predictability.html>
6. `core::iter::FusedIterator` documentation.
   <https://doc.rust-lang.org/std/iter/trait.FusedIterator.html>
7. `core::hash::Hash` documentation (notes on Hash-Eq consistency).
   <https://doc.rust-lang.org/std/hash/trait.Hash.html>
