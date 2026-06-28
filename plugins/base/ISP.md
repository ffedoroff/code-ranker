# ISP — Interface Segregation Principle

**TL;DR**: Clients should not be forced to depend on methods they do
not use. Prefer many small interfaces with focused responsibility
over one wide interface; let consumers ask for a `Read` capability
rather than a `ReadWriteSeek` one.

## Canonical sources

- Robert C. Martin, "The Interface Segregation Principle" (1996):
  <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>
- Martin, *Clean Architecture*, Ch. 10.
- Yoshua Wuyts, "Combinatorial purity" (small interfaces +
  composition):
  <https://blog.yoshuawuyts.com/combinatorial-purity/>
- The classic `Read`/`Write`/`Seek` factoring found in many standard
  libraries, where each is its own interface.

## The principle

An interface that bundles too many methods forces consumers to depend
on all of them even when only one is needed. Mock implementations,
test doubles, and runtime-polymorphic handles all pay the price of the
largest member of the interface.

Martin's original framing was in terms of Java/C# interfaces — large
interfaces caused implementors to leave methods unimplemented or to
throw an "unsupported operation" error. Even in languages where every
method must be defined, a fat interface produces the same trouble in a
different shape:

- Implementors stub methods that fail or panic at runtime.
- Implementors satisfy methods awkwardly (e.g. an `S3Storage`
  forced to implement `seek` because it shares an interface with
  files).
- Runtime-polymorphic handles become unwieldy because they expose
  the union of all methods, even those the call site never invokes.
- Test mocks become bloated.

ISP says: **fold a fat interface into several thin ones**, then let
each consumer declare exactly the surface it needs.

## Why it matters

In a project with many implementors, fat interfaces create a
**double bind**:

1. **Implementors are penalized** — they must implement every method
   even when only one is meaningful for their backend.
2. **Consumers are penalized** — they cannot use the interface in
   contexts where only one method is needed (e.g. a function that
   only reads cannot accept a read-only source because the interface
   demands write too).

ISP also has a strong interaction with [LSP](LSP.md):
small interfaces have small contracts that are easier to write down,
easier to test, and easier to honour. A 12-method interface has a
12-fold larger contract surface; an implementation that gets 11 right
and one slightly wrong still passes the type check.

ISP is the **interface counterpart of SRP**: SRP is about modules and
the actors they serve; ISP is about interfaces and the consumers they
serve.

## In practice

Good interface design rewards ISP-style factoring at every level.

### The I/O exemplar

```
interface Read:  read(buf) -> count
interface Write: write(buf) -> count
interface Seek:  seek(pos) -> position
interface BufRead extends Read: fill_buf() -> bytes  # ...
```

A function that copies bytes asks for a source that is `Read` and a
sink that is `Write`. A function that re-reads a header asks for
`Read + Seek`. A function that needs line-by-line input asks for
`BufRead`. Each consumer declares exactly the surface it needs; each
implementor implements only what its underlying resource supports.

A file implements all four because OS files support all four. A
network stream implements `Read + Write` but not `Seek`. A parser
written against `Read + Seek` cannot operate on a network stream,
which is the desired outcome — you cannot rewind a network socket.

### Extension interfaces

```
interface Storage: put(k, v)

interface StorageBatch extends Storage:
    put_batch(items):
        for (k, v) in items: this.put(k, v)   # default behaviour
```

`StorageBatch` is opt-in: callers depend on it only when they need
batching, and implementors get the default behaviour for free. The
core `Storage` interface stays small.

### Composable handles

```
function copy(r: Read, w: Write) -> count:  # ...
```

Each parameter exposes a single capability, so dispatch is cheap and
the function works for any combination of sources/sinks.

### Capability conjunction

A function that needs three capabilities lists them:

```
function replicate(s: Read + Seek + Send):  # ...
```

You did not have to define a `ReadSeekSend` interface. The
conjunction is ad-hoc and exactly describes the consumer's needs.

## Violations and remedies

### Anti-pattern: fat interface covering every backend feature

```
interface Database:
    query(sql) -> Rows
    execute(sql) -> count
    begin_transaction() -> Tx
    commit(tx)
    rollback(tx)
    migrate(m)
    dump() -> Bytes
    restore(b)
    vacuum()
    metrics() -> DbMetrics
    health() -> Health
    subscribe(channel) -> Receiver
```

A SQLite implementation is forced to fake `subscribe` (no pub/sub).
A read-only replica is forced to fake `execute`. A migration runner
that only needs `migrate` must accept the whole surface.

### Idiomatic fix: split by capability

```
interface Query:         query(sql) -> Rows
interface Execute:       execute(sql) -> count
interface Transactional: begin() -> Tx  # ...
interface Migratable:    migrate(m)
interface Backup:        dump() -> Bytes; restore(b)
interface Maintenance:   vacuum()
interface Observability: metrics() -> DbMetrics; health() -> Health
interface PubSub:        subscribe(ch) -> Receiver
```

A SQLite database implements `Query + Execute + Transactional +
Migratable + Backup + Maintenance + Observability`, but not `PubSub`.
A `read_only_replica()` returns something that is only `Query +
Observability`. The migration runner accepts anything `Migratable`.

If consumers commonly need three or four together, define a tiny
"prelude interface":

```
interface DatabaseFull extends Query + Execute + Transactional + Migratable {}
```

But avoid making `DatabaseFull` the *primary* interface — it should
be a convenience over the segregated parts.

### Anti-pattern: god `Service` interface

```
interface UserService:
    create(...) -> User
    deactivate(...)
    rotate_password(...)
    export_gdpr(...) -> Bytes
    send_welcome_email(...)
    assign_role(...)
```

A test that only needs `create` must mock all six methods. A
notification service that only consumes the `send_welcome_email`
capability must take a full `UserService` handle.

### Idiomatic fix: interfaces per use case

```
interface CreateUser:     create(...) -> User
interface DeactivateUser: deactivate(...)
interface RotatePassword: rotate_password(...)
interface GdprExport:     export(...) -> Bytes
interface WelcomeMailer:  welcome(...)
interface RoleAssigner:   assign(...)
```

The concrete `UserService` type implements all six; consumers take
the interface they actually need. Mocks become trivially small.

### Anti-pattern: stubbed-out method that fails at runtime

```
interface Cache:
    get(k) -> Optional<bytes>
    put(k, v)
    evict(k)
    evict_all()
    ttl_seconds() -> Optional<int>

FakeCacheForTest implements Cache:
    get(k):        return this.data.get(k)
    put(k, v):     this.data.insert(k, v)
    evict(k):      fail("not implemented")     # smell
    evict_all():   fail("not implemented")     # smell
    ttl_seconds(): return none
```

The failing stubs are runtime ISP debt — the interface is too broad
for the test.

### Idiomatic fix: split

```
interface CacheGet:   get(k) -> Optional<bytes>
interface CachePut:   put(k, v)
interface CacheEvict: evict(k); evict_all()
interface CacheTtl:   ttl_seconds() -> Optional<int>
```

The test fake implements only what the test needs (e.g. `CacheGet +
CachePut`).

## ISP at the package level

The same principle applies to **packages**: a package's public
surface should be focused. The classic anti-pattern is a "kitchen
sink" package (`utils`, `common`, `helpers`) that becomes a
dependency of everything and hard to update.

Apply ISP at the package level by splitting:

```
utils/                 ←  becomes  →   string_utils/
                                       time_utils/
                                       collection_utils/
```

Now a downstream package that needs only string helpers pulls only
`string_utils`, not the whole drawer.

## How code-ranker detects ISP violations

The structural signals:

| Signal | ISP interpretation |
|---|---|
| Interface with > N methods (high method-count) | Possible fat interface. Threshold tunable per project. Future rule. |
| Multiple implementations stubbing methods that fail/panic at runtime | Direct ISP smell. Requires AST inspection. Future rule. |
| Interface imported by many packages but only one method called from most call sites | Fan-out asymmetry — most callers want a smaller surface. (Requires call-graph aggregation per method, which code-ranker already has partially via fn nodes.) |
| Package consumed by N packages where each only uses 1-2 of the package's M public items | "Kitchen sink" package. Detectable from existing graph. |

A concrete future rule code-ranker could add:

**`fat-interface`**: interface has >= 7 public methods AND has >= 2
implementations across the project AND no segregated extension
interfaces exist. Severity: low / medium. Citation: this document +
Martin's ISP paper.

## Suggested recommendation template

> **ISP candidate**: interface `Database` exposes 12 methods and has 4
> implementations across the project. Several implementations fail at
> runtime for methods their backend cannot support. Split the
> interface into capability-segregated interfaces (`Query`, `Execute`,
> `Transactional`, `Migratable`, `Backup`, `PubSub`) and let each
> consumer ask for exactly the capabilities it needs. The classic
> `Read, Write, Seek, BufRead` I/O factoring is the canonical model.
>
> References:
>  - <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>

## Related principles

- [SRP](SRP.md) — SRP segregates *modules*;
  ISP segregates *interfaces*. They reinforce each other.
- [LSP](LSP.md) — small interfaces have small
  contracts; ISP makes LSP affordable.
- [DIP](DIP.md) — DIP wants consumers to
  depend on interfaces; ISP keeps those interfaces small enough to be
  worth depending on.
- [Composition Over Inheritance](CoI.md)
  — composing small capability requirements (`Read + Seek`) is the
  expression of "compose, don't inherit".

## References

1. Martin, R. C. "The Interface Segregation Principle". 1996.
   <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>
2. Martin, R. C. *Clean Architecture*. Ch. 10.
3. Wuyts, Y. "Combinatorial purity".
   <https://blog.yoshuawuyts.com/combinatorial-purity/>
