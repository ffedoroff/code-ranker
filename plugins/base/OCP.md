# OCP — Open/Closed Principle

**TL;DR**: A module is **open for extension** but **closed for
modification**. In practice this means: prefer adding a new
implementation, a new interface, or a new optional capability over
editing existing code paths; hide knobs behind extensible types,
sealed interfaces, and constrained construction.

## Canonical sources

- Bertrand Meyer, *Object-Oriented Software Construction* (1988):
  coined the principle in the inheritance-based form.
- Robert C. Martin, "The Open-Closed Principle" (1996, *C++ Report*):
  reframed for polymorphism rather than inheritance, the version most
  cited today. <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/ocp.pdf>
- Martin, *Clean Architecture* (2017), Ch. 8.

## The principle

A type, module, or package has fulfilled OCP when **its consumers can
add new behaviour without modifying its source**. Modification is
"reaching inside" — touching fields, adding branches to private
type discriminations, changing interface method signatures. Extension
is "plugging in" — implementing an interface the type publishes, adding
a new module that uses the type, or activating an optional capability.

The deep idea: any line of source you change is a line your existing
users might break on. So make new behaviour additive.

OCP is most often misread as "use inheritance" or "everything must be
abstract". Neither is true. The actual prescription is:

1. Identify the **axes of likely change**.
2. For each axis, expose an extension point that varies along it.
3. Keep everything else **closed** — don't allow callers to depend on
   internals that should be free to evolve.

In a typical package, the axes of likely change are usually:

- New variants of a discriminated type (logging output formats,
  network protocols, error kinds).
- New implementations of an interface (new storage backends, new
  authentication schemes).
- New optional fields in a config type (new flags, new tuning knobs).
- New parameters on a function (new context the caller can pass).

For each, there is an idiomatic "closed for modification" tool.

## Why it matters

OCP is the principle that protects you from **upstream cascades**:
a one-line change to a popular API ripples through every downstream
consumer at a version-breaking magnitude. A module with 50+ downstream
consumers must be designed to evolve additively or every release
blocks everyone who depends on it.

The opposite of OCP is *not* "no abstraction" — it is "every change
becomes a major version bump". You feel the absence of OCP through
release notes that say "BREAKING: renamed field; updated method
signature; added required parameter".

## Techniques

There are four sharp tools for OCP, available in some form in most
languages.

### 1. Closed types — restrict direct construction and exhaustive use

```
// A discriminated type whose set of variants may grow.
type DatabaseError =
    | Connection(ioError)
    | Query(text)
    | Migration(migrationFailure)
// marked "extensible": variants may be added later
```

Mark a type as extensible so that consumers outside the defining
module:

- Must include a catch-all branch when discriminating on it.
- Cannot construct it with a bare literal; only a constructor can.

Concretely: adding a variant (or a field) is no longer a breaking
change for downstream callers. You have **closed** the type for
exhaustive handling while **opening** it for additional variants.

### 2. Sealed interfaces — close who may implement

```
// Public interface users may *call*, but cannot *implement*.
internal interface Sealed {}
public interface Storage extends Sealed {
    put(key, value) -> Result
}

// Internal rule makes it actually sealed: only the defining
// module can satisfy Sealed.
```

A user can call `Storage.put` on anything that implements it, but
they cannot write their own implementation. You are free to add
methods to the interface without breaking external code — because no
external code implements it. The interface is **closed** for external
implementation while **open** for the defining module to add methods.

### 3. Constrained construction — close construction paths

```
type RequestBuilder<State> { /* state-specific fields */ }

type NoMethod
type WithMethod

// only available before a method is set
RequestBuilder<NoMethod>.method(m) -> RequestBuilder<WithMethod>

// only available after a method is set
RequestBuilder<WithMethod>.send() -> Future<Response>
```

`.send()` is only callable after `.method()`. Adding a new
**optional** step (e.g. `.timeout()`) does not change the state
sequence; adding a new **required** step would (so reserve this
technique for genuinely required steps).

### 4. A façade module — close the import paths

```
// package foo, public entry point
module internal          // private
module another_internal  // private

// re-export only the intended public surface
export internal.PublicApi
export another_internal.OtherApi
```

Consumers depend on `foo.PublicApi`, not on `foo.internal.PublicApi`.
You can rename or move `internal` without breaking anyone.

(See [DIP](DIP.md) and the note in
[Composition](CoI.md) about avoiding blanket
re-exports — those are the *un*closed kind.)

## Violations and remedies

### Anti-pattern: exhaustively handling a foreign discriminated type

```
// In your module
import EventKind from foreign_package

function dispatch(e) {
    switch e {
        case EventKind.Insert: /* ... */
        case EventKind.Update: /* ... */
        case EventKind.Delete: /* ... */
        // Foreign package adds EventKind.Truncate in 1.4.0 — your code
        // breaks because there is no catch-all branch.
    }
}
```

### Idiomatic fix: defensively add a catch-all or own the dispatch

```
function dispatch(e) {
    switch e {
        case EventKind.Insert: /* ... */
        case EventKind.Update: /* ... */
        case EventKind.Delete: /* ... */
        default: default_handler(e)   // open to upstream additions
    }
}
```

For your own types you expect to grow, mark them extensible so
callers are forced into the catch-all pattern.

### Anti-pattern: a config type whose fields are all public and required

```
type ConnectionOptions {
    host: text
    port: int
    timeout: Duration
}
```

Adding a field is a breaking change because all the call sites that
build `ConnectionOptions { host, port, timeout }` lack the new field.

### Idiomatic fix: a builder plus an extensible type

```
// extensible: cannot be literal-constructed from outside
type ConnectionOptions {
    host: text
    port: int
    timeout: Duration
}

ConnectionOptions.new(host) -> ConnectionOptions {
    return { host, port: 5432, timeout: 30s }
}
ConnectionOptions.port(p) -> self   // chainable setter
ConnectionOptions.timeout(t) -> self
```

Adding `retries` later is non-breaking — the type cannot be
literal-constructed externally, and the builder gains a new method.

### Anti-pattern: an interface that downstream code implements, then you add methods

```
interface Cache {
    get(k) -> Option<bytes>
    put(k, v)
}
```

Six downstream packages each provide their own `Cache` implementation.
You realize you need eviction control and add `evict(k)`. Every
downstream implementation fails to compile.

### Idiomatic fix: seal the interface

```
internal interface Sealed {}

public interface Cache extends Sealed {
    get(k) -> Option<bytes>
    put(k, v)
}

// only the defining module marks types as Sealed:
InMemoryCache implements Sealed, Cache { /* ... */ }
RedisCache    implements Sealed, Cache { /* ... */ }
```

External code can call `cache.get(k)` but cannot implement `Cache`
for their own types. Adding `evict` is now an additive change inside
the defining module.

If external implementations are part of the value proposition (e.g.
a plugin system), do NOT seal — instead, provide a default
implementation for new methods so older implementations still work:

```
interface Cache {
    get(k) -> Option<bytes>
    put(k, v)
    // default method: opt-in, so existing implementors still satisfy
    evict(k) { self.put(k, empty) }
}
```

This keeps the *unsealed* interface closed for breakage at the cost
of giving authors a (sometimes wrong) default.

### Anti-pattern: hardcoded variant dispatch in business logic

```
function render(format, data) -> text {
    switch format {
        case Format.Json: return render_json(data)
        case Format.Toml: return render_toml(data)
        case Format.Yaml: return render_yaml(data)
    }
}
```

Adding `Format.Cbor` modifies `render`. Every place that performs a
switch on `Format` is a modification point.

### Idiomatic fix: interface plus registry

```
interface Renderer {
    render(data) -> text
    format_id() -> text
}

type RendererRegistry {
    renderers: list of Renderer
}

RendererRegistry.register(r)    { self.renderers.add(r) }
RendererRegistry.render(fmt, data) -> Option<text> {
    return self.renderers.find(r -> r.format_id() == fmt)
                         .map(r -> r.render(data))
}
```

A new format is a new implementation, registered at startup.
`RendererRegistry` itself does not change.

## OCP at the package level

The strongest form of OCP at the package boundary is preserving
**type identity across versions**. If an old package version
re-exports the type from a new package version, the two names refer
to the *same type*:

```
// old_package v2 (still maintained for stragglers)
// depends on new_package v3 and re-exports its type
export new_package.Item
```

`old_package.Item` and `new_package.Item` are now the *same type*.
Downstream code on the old package can still interoperate with code on
the new package because the type identity is preserved. The pattern
opens a path for additive evolution across major-version boundaries.

## How code-ranker detects OCP violations

OCP violations are subtler than SRP — they often look like normal
code until upstream-evolution time. Code Ranker can flag the structural
*precursors*:

| Signal | OCP interpretation |
|---|---|
| Public interface with N implementations across multiple packages | If unsealed, every method addition is breaking. The `high-fan-in-public-api` rule already flags hotspots; OCP advice is to seal. |
| Public discriminated type without an extensibility marker, handled in many places | Same hazard for variant addition. Code Ranker's `node_visibility` plus a cross-package match-count would catch this in a future rule. |
| Public type with literal-construction sites across packages | Same hazard for field addition. |
| Blanket glob re-exports | Closes nothing — every public item of the source becomes part of *your* contract; you cannot rename them without breaking. |

Cross-references in code-ranker's catalog:

- `high-fan-in-public-api` already prescribes sealed interfaces and
  extensible types. Severity escalates when the API is unsealed.
- A future `unsealed-public-interface` rule would directly map.

## Suggested recommendation template

> **OCP candidate**: interface `Cache` is public and has 6
> implementations across the project. Adding a method to the interface
> currently breaks all 6 implementors. Seal the interface via a private
> supertype if external implementations are not part of the value
> proposition; otherwise mark types as extensible and use
> default-implemented methods when extending.

## Related principles

- [SRP](SRP.md) — splits before OCP defends.
- [LSP](LSP.md) — defines what "extension" means
  precisely: a substitute that behaves like the base.
- [DIP](DIP.md) — provides the interface-based
  extension point OCP demands.

## References

1. Meyer, B. *Object-Oriented Software Construction*. 1988.
2. Martin, R. C. "The Open-Closed Principle". *C++ Report*, 1996.
   <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/ocp.pdf>
3. Martin, R. C. *Clean Architecture*. Prentice Hall, 2017. Ch. 8.
