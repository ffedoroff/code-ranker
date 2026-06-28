# DRY — Don't Repeat Yourself

**TL;DR**: Every piece of knowledge must have a single, unambiguous,
authoritative representation within a system. DRY is about **knowledge
duplication**, not **code duplication** — copy-pasted lines that
encode different decisions are not DRY violations; one line in two
different modules that means "the maximum retry count" is.

## Canonical sources

- Andy Hunt and Dave Thomas, *The Pragmatic Programmer* (1999,
  Addison-Wesley): the source of the principle's name. Topic 9 in
  the 20th-anniversary edition: <https://pragprog.com/titles/tpp20/>
- Andy Hunt blog, "DRY is About Knowledge" (2014):
  <https://blog.codinghorror.com/dry-not-just-about-code/> (Atwood
  citing Hunt)
- Dan Abramov, "The WET Codebase":
  <https://overreacted.io/the-wet-codebase/> (counterpoint:
  premature DRY is worse than duplication)
- Sandi Metz, "The Wrong Abstraction" (2016):
  <https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction>

## The principle

The Pragmatic Programmer text:

> Every piece of knowledge must have a single, unambiguous,
> authoritative representation within a system.

The misreading the authors regret most: DRY is not "don't write the
same characters twice". It is "don't encode the same **decision** in
two places where they can drift apart".

Hunt later clarified: if two pieces of code happen to look identical
**because the underlying concept happens to coincide right now**, that
is not a DRY violation. It is *accidental duplication*. Extracting it
into a shared abstraction creates a worse problem — you have welded
two concepts together that are free to diverge later, and the
abstraction will fight every change.

Real DRY violations are about **knowledge**: a constant, a regex, a
business rule, a calculation, a schema. When the regulation says
"customers under 18 cannot purchase alcohol", the number `18` should
appear in exactly one place in your code.

## Why it matters

When the same knowledge lives in N places:

- Updates require finding all N. You will miss some.
- Tests may pass on the locations you remembered and silently fail
  in production for the ones you forgot.
- Reviewers cannot tell whether N differences are intentional or are
  drift.
- Onboarding becomes harder: "Where is the truth about X?" has N
  answers.

When *accidental* duplication is force-extracted (the "wrong
abstraction" failure mode), N use sites are forced to evolve together
when they actually need to diverge. The abstraction grows boolean
flags, special cases, and conditionals until it is harder to read
than the original duplication.

The skill is distinguishing knowledge duplication (which DRY targets)
from accidental similarity (which DRY does not).

## Mechanisms and temptations

Most languages offer several mechanisms that make true DRY clean and
several that make false DRY tempting. Use the first set; resist the
second.

### Mechanisms for genuine DRY

**Named constants**:

```
public const MIN_ALCOHOL_AGE = 18
public const MAX_USERNAME_LEN = 64
public const PASSWORD_RESET_TTL = minutes(15)
```

One canonical place. A misspelled reference to a named constant
becomes an error the moment you read or run the code — far safer
than a stray literal.

**Functions that name a calculation**:

```
function effective_tax_rate(subtotal, jurisdiction):
    return base_rate(jurisdiction) + surcharge_for(subtotal)
```

The formula has one expression. If the regulation changes, you
change one place.

**Generic / parameterized functions for true polymorphism**:

```
function parse_id(s) -> Id:
    return Uuid.parse(s).map(to_id)
```

Used to derive `UserId`, `OrderId`, `TransactionId` from the same
parsing logic — *which is genuinely the same knowledge*.

**Code generation or templating for textual repetition with knowledge content**:

When many types share the exact same shape, generate them from one
template instead of hand-writing each:

```
generate_id_type(UserId)
generate_id_type(OrderId)
generate_id_type(TransactionId)
```

The template encodes the **decision** "all IDs are UUIDs with this
exact shape". If the decision changes (say, to ULIDs), one
modification updates all of them.

**Shared cross-cutting capabilities**:

A serialization or debug-printing facility codifies "every domain
type gets these capabilities" once, in one place. You do not
re-implement printing or serialization for every type.

**Type aliases for shared shapes**:

```
type ConfigResult = Result<T, ConfigError>
```

The fact that "config operations return a result over `ConfigError`"
appears once.

### Mechanisms that *tempt* false DRY

**Over-eager helper extraction**:

```
function validate_user_input(s):
    return length(s) > 0 and length(s) < 100 and not contains(s, "\0")

function validate_order_note(s):
    return length(s) > 0 and length(s) < 100 and not contains(s, "\0")
```

Tempting to extract `validate_short_text(s)`. But the two validations
*happen* to coincide today. Tomorrow the order note rule changes to
"<= 500 chars" and now the helper grows a parameter, a boolean flag,
two variants, etc.

Better: leave them duplicated until the third copy appears. Hunt:
"Rule of Three" — abstract when you have *three* concrete instances
proving the abstraction is real, not two.

**Premature shared package**:

Multi-package projects accumulate a `common/` or `utils/` package
that becomes a junk drawer of weakly-related helpers. The package's
"DRY" benefit is illusory — the helpers were never the same
knowledge, just the same shape.

Better: leave the local helpers local. If three packages genuinely
need the same calculation, extract *that calculation*, not "stuff
the three packages might share".

**Forcing identical APIs onto different abstractions**:

```
interface Storage:
    put(k, v)
    get(k) -> Optional<bytes>

HashMap implements Storage   # ...
S3Client implements Storage  # ...
```

Memory and S3 do not share a contract (see
[LSP](LSP.md)). The shared interface is a
DRY-shaped illusion masking incompatible behaviours.

## Violations and remedies

### Anti-pattern: magic numbers duplicated

```
# api/handlers/auth
if length(username) > 64: return error(TooLong)

# domain/user
if length(request.name) > 64: return error(Invalid)

# admin/forms
function validate(s): return length(s) <= 64
```

If the limit changes, three places must be edited and someone will
miss the third.

### Idiomatic fix: single source of truth in a domain module

```
# domain/limits
public const MAX_USERNAME_LEN = 64
```

```
# everywhere else
import MAX_USERNAME_LEN from domain.limits
if length(username) > MAX_USERNAME_LEN: ...
```

### Anti-pattern: duplicated SQL schema knowledge

```
# repo/users
const COLS = "id, email, name, created_at, deleted_at"

function fetch(...):
    query("SELECT id, email, name, created_at, deleted_at FROM users WHERE id = ?")

function insert(...):
    query("INSERT INTO users (id, email, name, created_at) VALUES ...")
```

The column list appears three times (in `COLS`, in the SELECT, in
the INSERT). Adding a column requires updating each.

### Idiomatic fix: a single row mapping plus one column list

```
type UserRow:
    id, email, name, created_at, deleted_at

const COLS = "id, email, name, created_at, deleted_at"

function fetch(id) -> UserRow:
    query_as(UserRow, "SELECT " + COLS + " FROM users WHERE id = ?", id)
```

Adding a column means: add a field to `UserRow`, add a name to
`COLS`. Two edits, both in the same file.

### Anti-pattern: parallel validation in API and domain

```
# api/handlers/orders
function create_order_handler(req):
    if is_empty(req.items): return reject("no items")
    if req.total < 0: return reject("negative total")
    # ... 12 more checks ...

# domain/order
function Order.new(items, total):
    if is_empty(items): return error(...)
    if total < 0: return error(...)
    # ... 12 more checks ...
```

Every validation rule exists twice. They drift.

### Idiomatic fix: validation lives in the domain; API delegates

```
# domain/order
function Order.new(items, total) -> Result<Order, DomainError>:
    if is_empty(items): return error(NoItems)
    if total < 0: return error(NegativeTotal)
    # ...
```

```
# api/handlers/orders
function create_order_handler(req):
    order = Order.new(req.items, req.total)
    if order is error: return reject(order.error)
    # ...
```

The API performs *no business validation*. It translates errors. A
new rule is added in one place — in the domain.

### Anti-pattern: copy-pasted code that ISN'T DRY

```
function calculate_tax_us(amount): return amount * 0.07
function calculate_tax_eu(amount): return amount * 0.21
function calculate_tax_uk(amount): return amount * 0.20
```

It would be tempting to extract `calculate_tax(rate, amount)`.
Should you?

**No** — for two reasons:

1. The three tax rates are not the same knowledge. They are
   independent regulations. If the EU rate changes, the US rate is
   unaffected.
2. The functions communicate intent. `calculate_tax_us(amount)` reads
   better at the call site than `calculate_tax(0.07, amount)`.

When VAT rates split by region into 27 individual values that vary
together (per EU directive), THEN extract. The Rule of Three applies.

### Idiomatic fix: leave as-is

Resist the urge. Three lookups in a table is fine. (See Sandi Metz,
"The Wrong Abstraction".)

## DRY at the package level

Cross-package DRY shows up as:

- A constant duplicated in multiple package manifests (e.g. the
  version). Fix: declare it once at the top of a multi-package
  project and have packages inherit it.
- A type duplicated across packages because both need "the same"
  shape. Fix: one defining package, the others depend on it. (Or
  *don't fix* if the two types happen to look alike but mean
  different things.)
- A dependency version pinned in multiple manifests. Fix: declare
  shared dependency versions once for the whole project.

## How code-ranker detects DRY violations

DRY is the hardest principle to detect automatically — knowledge
duplication does not have a graph signature. Code Ranker can flag
*candidates*:

| Signal | DRY interpretation |
|---|---|
| Identical function names across multiple modules (e.g. `validate`, `parse`, `format`) | Possible knowledge duplication. Requires fn-name overlap analysis. |
| Public constants with identical *values* across multiple packages | Strong DRY-violation candidate. Requires AST inspection. |
| Multiple packages with similar dependency lists | Possibly the same domain repeated. |
| Repeated string-literal regex patterns | Regex literals appearing in N source files is a textbook DRY violation. |

Code Ranker's static graph cannot tell you whether two functions
*encode the same knowledge* — that requires understanding the
function bodies. A future rule could flag literal duplication and
let the LLM-verification step (see `cpt-code-ranker-fr-prompt-composer`)
decide.

## Suggested recommendation template

> **DRY candidate** (low confidence): the constant `64` appears as a
> max-length check in 5 places across the project (api/auth,
> domain/user, admin/forms, infra/email/templates,
> shared/limits). If these are encoding the same business rule
> ("usernames must be <= 64 chars"), consolidate to a single
> `domain.limits.MAX_USERNAME_LEN`. If they are independent (a
> column width, an email subject limit, a UI hint), keep them
> separate.
>
> Code Ranker cannot tell which case applies. See *Pragmatic Programmer*
> Topic 9 for guidance on the call.

## Related principles

- [KISS](KISS.md) — DRY can violate KISS when premature abstraction
  introduces a more complex shape than the duplication.
- [YAGNI](YAGNI.md) — don't DRY for a hypothetical second instance
  that may never appear.
- [SRP](SRP.md) — SRP is the discipline that
  produces *true* DRY by aligning code-units with reasons-to-change.

## References

1. Hunt, A. and Thomas, D. *The Pragmatic Programmer: From Journeyman
   to Master*. Addison-Wesley, 1999 (20th anniv. ed., 2019).
   <https://pragprog.com/titles/tpp20/>
2. Abramov, D. "The WET Codebase".
   <https://overreacted.io/the-wet-codebase/>
3. Metz, S. "The Wrong Abstraction". 2016.
   <https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction>
4. Atwood, J. "DRY: It's About Knowledge". 2014.
   <https://blog.codinghorror.com/dry-not-just-about-code/>
