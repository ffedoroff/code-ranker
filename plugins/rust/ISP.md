# ISP — Interface Segregation Principle (in Rust)

**TL;DR**: Clients should not be forced to depend on methods they do
not use. In Rust: prefer many small traits with focused responsibility
over one wide trait; let consumers ask for `impl Read` rather than
`impl ReadWriteSeek`.

## Canonical sources

- Robert C. Martin, "The Interface Segregation Principle" (1996):
  <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>
- Martin, *Clean Architecture*, Ch. 10.
- Yoshua Wuyts, "Combinatorial purity" (small traits + composition):
  <https://blog.yoshuawuyts.com/combinatorial-purity/>
- Rust standard library: the `Read`/`Write`/`Seek` factoring (each
  is its own trait).
  <https://doc.rust-lang.org/std/io/trait.Read.html>
- niko matsakis, "Async fn in trait" (extension trait composition):
  <https://smallcultfollowing.com/babysteps/blog/2023/05/03/dyn-async-traits-part-11/>
- Rust API Guidelines, "Flexibility":
  <https://rust-lang.github.io/api-guidelines/flexibility.html>

## The principle

A trait that bundles too many methods forces consumers to depend on
all of them even when only one is needed. Mock implementations,
test doubles, and trait objects all pay the price of the largest
member of the trait.

Martin's original framing was in terms of Java/C# interfaces — large
interfaces caused implementors to leave methods unimplemented or to
throw `UnsupportedOperationException`. Rust does not allow either:
every method must compile. So the Rust failure mode is different but
worse:

- Implementors stub methods with `unimplemented!()`, which panics at
  runtime.
- Implementors satisfy methods awkwardly (e.g. an `S3Storage`
  forced to implement `seek` because it shares a trait with files).
- Trait objects (`dyn Trait`) become unwieldy because they expose
  the union of all methods, even those the call site never invokes.
- Test mocks become bloated.

ISP says: **fold a fat trait into several thin ones**, then let
each consumer declare exactly the surface it needs.

## Why it matters

In a workspace with many implementors, fat traits create a
**double bind**:

1. **Implementors are penalized** — they must implement every method
   even when only one is meaningful for their backend.
2. **Consumers are penalized** — they cannot use the trait in
   contexts where only one method is needed (e.g. a function that
   only `Read`s cannot accept a `Read`-only sink because the trait
   demands `Write` too).

ISP also has a strong interaction with [LSP](LSP.md):
small traits have small contracts that are easier to write down,
easier to test, and easier to honour. A 12-method trait has a 12-fold
larger contract surface; an `impl` that gets 11 right and one
slightly wrong still passes the type check.

In Rust specifically, ISP is the **trait counterpart of SRP**: SRP
is about modules and the actors they serve; ISP is about traits and
the consumers they serve.

## In Rust

Rust's design rewards ISP-style factoring at every level.

### The std::io exemplar

```rust
pub trait Read { fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>; }
pub trait Write { fn write(&mut self, buf: &[u8]) -> io::Result<usize>; }
pub trait Seek { fn seek(&mut self, pos: SeekFrom) -> io::Result<u64>; }
pub trait BufRead: Read { fn fill_buf(&mut self) -> io::Result<&[u8]>; /* ... */ }
```

A function that copies bytes asks for `R: Read, W: Write`. A function
that re-reads a header asks for `R: Read + Seek`. A function that
needs line-by-line input asks for `R: BufRead`. Each consumer
declares exactly the surface it needs; each implementor implements
only what its underlying resource supports.

`File` implements all four because OS files support all four. A
`TcpStream` implements `Read + Write` but not `Seek`. A network
parser written against `Read + Seek` does not compile against
`TcpStream`, which is the desired outcome — you cannot rewind a
network socket.

### Extension traits

```rust
pub trait Storage { fn put(&self, k: &str, v: &[u8]); }

pub trait StorageBatchExt: Storage {
    fn put_batch(&self, items: &[(&str, &[u8])]) {
        for (k, v) in items { self.put(k, v); }
    }
}
impl<S: Storage> StorageBatchExt for S {}
```

`StorageBatchExt` is opt-in: callers `use StorageBatchExt` only when
they need batching, and implementors get a default impl for free.
The core `Storage` trait stays small.

### Composable trait objects

```rust
fn copy(r: &mut dyn Read, w: &mut dyn Write) -> io::Result<u64> { /* ... */ }
```

Each `dyn` is small (one vtable method), so dispatch is cheap and
the function works for any combination of sources/sinks.

### Generic-bound conjunction

A function that needs three capabilities lists them:

```rust
fn replicate<S: Read + Seek + Send + 'static>(s: S) { /* ... */ }
```

You did not have to define `ReadSeekSendStatic`. The conjunction is
ad-hoc and exactly describes the consumer's needs.

## Violations and remedies

### Anti-pattern: fat trait covering every backend feature

```rust
pub trait Database {
    fn query(&self, sql: &str) -> Result<Rows>;
    fn execute(&self, sql: &str) -> Result<u64>;
    fn begin_transaction(&self) -> Result<Tx>;
    fn commit(&self, tx: Tx) -> Result<()>;
    fn rollback(&self, tx: Tx) -> Result<()>;
    fn migrate(&self, m: &Migration) -> Result<()>;
    fn dump(&self) -> Result<Bytes>;
    fn restore(&self, b: Bytes) -> Result<()>;
    fn vacuum(&self) -> Result<()>;
    fn metrics(&self) -> DbMetrics;
    fn health(&self) -> Health;
    fn subscribe(&self, channel: &str) -> Receiver<Notification>;
}
```

A `SqliteDatabase` impl is forced to fake `subscribe` (no pub/sub).
A read-only replica is forced to fake `execute`. A migration runner
that only needs `migrate` must accept the whole surface.

### Idiomatic fix: split by capability

```rust
pub trait Query { fn query(&self, sql: &str) -> Result<Rows>; }
pub trait Execute { fn execute(&self, sql: &str) -> Result<u64>; }
pub trait Transactional { fn begin(&self) -> Result<Tx>; /* ... */ }
pub trait Migratable { fn migrate(&self, m: &Migration) -> Result<()>; }
pub trait Backup { fn dump(&self) -> Result<Bytes>; fn restore(&self, b: Bytes) -> Result<()>; }
pub trait Maintenance { fn vacuum(&self) -> Result<()>; }
pub trait Observability { fn metrics(&self) -> DbMetrics; fn health(&self) -> Health; }
pub trait PubSub { fn subscribe(&self, ch: &str) -> Receiver<Notification>; }
```

`SqliteDatabase` implements `Query + Execute + Transactional + Migratable + Backup + Maintenance + Observability`, but not `PubSub`.
A `read_only_replica()` returns `impl Query + Observability` and
nothing else. The migration runner accepts `impl Migratable`.

If consumers commonly need three or four together, define a tiny
"prelude trait":

```rust
pub trait DatabaseFull: Query + Execute + Transactional + Migratable {}
impl<T: Query + Execute + Transactional + Migratable> DatabaseFull for T {}
```

But avoid making `DatabaseFull` the *primary* trait — it should be
a convenience over the segregated parts.

### Anti-pattern: god `Service` trait

```rust
pub trait UserService {
    fn create(&self, ...) -> Result<User>;
    fn deactivate(&self, ...) -> Result<()>;
    fn rotate_password(&self, ...) -> Result<()>;
    fn export_gdpr(&self, ...) -> Result<Bytes>;
    fn send_welcome_email(&self, ...) -> Result<()>;
    fn assign_role(&self, ...) -> Result<()>;
}
```

A test that only needs `create_user` must mock all six methods. A
notification service that only consumes the `send_welcome_email`
capability must take a full `Box<dyn UserService>`.

### Idiomatic fix: traits per use case

```rust
pub trait CreateUser { fn create(&self, ...) -> Result<User>; }
pub trait DeactivateUser { fn deactivate(&self, ...) -> Result<()>; }
pub trait RotatePassword { fn rotate_password(&self, ...) -> Result<()>; }
pub trait GdprExport { fn export(&self, ...) -> Result<Bytes>; }
pub trait WelcomeMailer { fn welcome(&self, ...) -> Result<()>; }
pub trait RoleAssigner { fn assign(&self, ...) -> Result<()>; }
```

The concrete `UserService` struct implements all six; consumers
take the trait they actually need. Mocks become trivially small.

### Anti-pattern: trait method `unimplemented!()`

```rust
pub trait Cache {
    fn get(&self, k: &str) -> Option<Vec<u8>>;
    fn put(&self, k: &str, v: Vec<u8>);
    fn evict(&self, k: &str);
    fn evict_all(&self);
    fn ttl_seconds(&self) -> Option<u64>;
}

impl Cache for FakeCacheForTest {
    fn get(&self, k: &str) -> Option<Vec<u8>> { self.data.get(k).cloned() }
    fn put(&self, k: &str, v: Vec<u8>) { self.data.insert(k.into(), v); }
    fn evict(&self, _: &str) { unimplemented!() }     // smell
    fn evict_all(&self) { unimplemented!() }           // smell
    fn ttl_seconds(&self) -> Option<u64> { None }
}
```

The `unimplemented!()` calls are runtime ISP debt — the trait is too
broad for the test.

### Idiomatic fix: split

```rust
pub trait CacheGet { fn get(&self, k: &str) -> Option<Vec<u8>>; }
pub trait CachePut { fn put(&self, k: &str, v: Vec<u8>); }
pub trait CacheEvict {
    fn evict(&self, k: &str);
    fn evict_all(&self);
}
pub trait CacheTtl { fn ttl_seconds(&self) -> Option<u64>; }
```

The test fake implements only what the test needs (e.g. `CacheGet + CachePut`).

## ISP at the workspace level

The same principle applies to **crates**: a crate's public surface
should be focused. The classic anti-pattern is a "kitchen sink"
crate (`utils`, `common`, `helpers`) that becomes a dependency of
everything and hard to update.

Apply ISP at the crate level by splitting:

```
utils/                 ←  becomes  →   string_utils/
                                       time_utils/
                                       collection_utils/
```

Now a downstream crate that needs only string helpers pulls only
`string_utils`, not the whole drawer.

## How code-ranker detects ISP violations

The structural signals:

| Signal | ISP interpretation |
|---|---|
| Trait with > N methods (high method-count) | Possible fat trait. Threshold tunable per project. Future rule. |
| Multiple impls calling `unimplemented!()` / `todo!()` in trait method bodies | Direct ISP smell. Requires AST inspection. Future rule. |
| Trait imported by many crates but only one method called from most call sites | Fan-out asymmetry — most callers want a smaller surface. (Requires call-graph aggregation per method, which code-ranker already has partially via fn nodes.) |
| Crate consumed by N crates where each only uses 1-2 of the crate's M public items | "Kitchen sink" crate. Detectable from existing graph. |

A concrete future rule code-ranker could add:

**`fat-trait`**: trait has ≥ 7 public methods AND has ≥ 2
implementations across the workspace AND no segregated extension
traits exist. Severity: low / medium. Citation: this document +
Martin's ISP paper.

## Suggested recommendation template

> **ISP candidate**: trait `Database` exposes 12 methods and has 4
> implementations across the workspace. Several implementations panic
> with `unimplemented!()` for methods their backend cannot support.
> Split the trait into capability-segregated traits (`Query`, `Execute`,
> `Transactional`, `Migratable`, `Backup`, `PubSub`) and let each
> consumer ask for exactly the capabilities it needs. The Rust std
> `io::{Read, Write, Seek, BufRead}` factoring is the canonical model.
>
> References:
>  - <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>
>  - <https://doc.rust-lang.org/std/io/index.html>

<!-- doc:base "Related principles" -->

## References

1. Martin, R. C. "The Interface Segregation Principle". 1996.
   <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>
2. Martin, R. C. *Clean Architecture*. Ch. 10.
3. Wuyts, Y. "Combinatorial purity".
   <https://blog.yoshuawuyts.com/combinatorial-purity/>
4. Rust standard library `std::io` documentation.
   <https://doc.rust-lang.org/std/io/>
5. Rust API Guidelines, "Flexibility".
   <https://rust-lang.github.io/api-guidelines/flexibility.html>
6. niko matsakis, "dyn async traits, part 11".
   <https://smallcultfollowing.com/babysteps/blog/2023/05/03/dyn-async-traits-part-11/>
