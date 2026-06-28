# DRY — Don't Repeat Yourself (in TypeScript)

**TL;DR**: Every piece of knowledge must have a single, unambiguous,
authoritative representation within a system. DRY is about **knowledge
duplication**, not **code duplication** — copy-pasted lines that
encode different decisions are not DRY violations; one line in two
different modules that means "the maximum retry count" is. In a
TypeScript codebase the most common offenders are duplicated Zod
schemas, parallel domain/wire types, hand-rolled `try/catch` around
every `fetch`, and React hooks copy-pasted instead of extracted.

## Canonical sources

- Andy Hunt and Dave Thomas, *The Pragmatic Programmer* (1999,
  Addison-Wesley): the source of the principle's name. Topic 9 in
  the 20th-anniversary edition: <https://pragprog.com/titles/tpp20/>
- Andy Hunt blog, "DRY is About Knowledge" (2014):
  <https://blog.codinghorror.com/dry-not-just-about-code/> (Atwood
  citing Hunt)
- matklad, "Three Levels of Repetition" (2024):
  <https://matklad.github.io/2024/02/02/three-levels-of-repetition.html>
- Dan Abramov, "The WET Codebase" (2019): an explicit anti-DRY
  argument from a React-ecosystem authority.
  <https://overreacted.io/the-wet-codebase/>
- Sandi Metz, "The Wrong Abstraction" (2016): "duplication is far
  cheaper than the wrong abstraction".
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
abstraction will fight every change. Sandi Metz puts the corollary
bluntly: *duplication is far cheaper than the wrong abstraction*.

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
than the original duplication. Abramov's "WET Codebase" talk is the
canonical warning for the JavaScript/TypeScript world: most "DRY"
extractions in component code are coupling disguised as reuse.

The skill is distinguishing knowledge duplication (which DRY targets)
from accidental similarity (which DRY does not).

## In TypeScript

TypeScript has several mechanisms that make true DRY clean and
several that make false DRY tempting. Use the first set; resist the
second.

### Mechanisms for genuine DRY

**Constants with `as const`**:

```ts
export const MIN_ALCOHOL_AGE = 18 as const;
export const MAX_USERNAME_LEN = 64 as const;
export const PASSWORD_RESET_TTL_MS = 15 * 60 * 1000;
```

One canonical place. `as const` gives the literal type, so misuse at
call sites is a compile error rather than a silent number.

**Functions that name a calculation**:

```ts
export function effectiveTaxRate(
  subtotal: Money,
  jurisdiction: Jurisdiction,
): Rate {
  return baseRate(jurisdiction) + surchargeFor(subtotal);
}
```

The formula has one expression. If the regulation changes, you
change one place.

**Generic functions for true polymorphism**:

```ts
export function parseId<T extends { readonly __brand: symbol }>(
  s: string,
  brand: (u: string) => T,
): T {
  if (!UUID_RE.test(s)) throw new ParseError(`bad uuid: ${s}`);
  return brand(s);
}
```

Used to derive `UserId`, `OrderId`, `TransactionId` from the same
parsing logic — *which is genuinely the same knowledge*.

**`as const` + generic factory functions** (the TS analogue of
`macro_rules!`):

```ts
function makeIdBrand<Name extends string>(name: Name) {
  type Branded = string & { readonly __brand: Name };
  return {
    parse: (s: string): Branded => {
      if (!UUID_RE.test(s)) throw new ParseError(`bad ${name}: ${s}`);
      return s as Branded;
    },
    is: (s: unknown): s is Branded =>
      typeof s === "string" && UUID_RE.test(s),
  };
}

export const UserId = makeIdBrand("UserId");
export const OrderId = makeIdBrand("OrderId");
export const TransactionId = makeIdBrand("TransactionId");
export type UserId = ReturnType<typeof UserId.parse>;
export type OrderId = ReturnType<typeof OrderId.parse>;
```

The factory encodes the **decision** "all IDs are branded UUIDs with
this exact shape". If the decision changes (say, to ULIDs), one
modification updates all newtypes. For text-level codegen beyond what
generics can express, a build script driving `tsc` programmatically
or a custom transformer plugin is the escape hatch — but reach for it
last.

**Zod schemas as single source of truth**:

```ts
import { z } from "zod";

export const User = z.object({
  id: z.string().uuid(),
  email: z.string().email(),
  name: z.string().min(1).max(MAX_USERNAME_LEN),
  createdAt: z.coerce.date(),
  deletedAt: z.coerce.date().nullable(),
});
export type User = z.infer<typeof User>;
```

The schema *is* the type *is* the validator *is* the wire-format
contract. One edit propagates everywhere via `z.infer`.

**Type aliases for shared shapes**:

```ts
export type ConfigResult<T> = { ok: true; value: T } | { ok: false; error: ConfigError };
```

The fact that "config operations return this discriminated union"
appears once.

**Discriminated unions for error knowledge**:

```ts
export class DomainError extends Error {
  constructor(public readonly kind: "NoItems" | "NegativeTotal" | "Forbidden") {
    super(kind);
  }
}
```

One enumeration of failure modes, exhaustively checkable at every
`switch`.

### Mechanisms that *tempt* false DRY

**Over-eager helper extraction**:

```ts
const validateUserInput = (s: string) => s.length > 0 && s.length < 100 && !s.includes("\0");
const validateOrderNote  = (s: string) => s.length > 0 && s.length < 100 && !s.includes("\0");
```

Tempting to extract `validateShortText`. But the two validations
*happen* to coincide today. Tomorrow the order note rule changes to
"≤ 500 chars" and now the helper grows a parameter, a boolean flag,
two enum variants, etc.

Better: leave them duplicated until the third copy appears. Hunt:
"Rule of Three" — abstract when you have *three* concrete instances
proving the abstraction is real, not two.

**Type-level over-DRY** (a TS-specific trap):

```ts
type DeepPartialReadonlyKeysExcept<T, K extends keyof T> = /* 40 lines of infer */;
```

Mapped types, conditional types, and `infer` give you enough rope to
encode an entire DSL in the type system. The temptation to
"unify all our DTO transformations into one type" produces types no
one can read and errors no one can decode. The compiler is not your
audience — your colleagues are. Three concrete `Partial<Pick<...>>`
declarations beat one `MagicTransform<T, K, M, F>` every time.

**Premature shared package**:

Monorepos accumulate a `packages/common/` or `packages/utils/` that
becomes a junk drawer of weakly-related helpers. The package's "DRY"
benefit is illusory — the helpers were never the same knowledge,
just the same shape.

Better: leave the local helpers local. If three packages genuinely
need the same calculation, extract *that calculation*, not "stuff
the three packages might share".

**Barrel imports that re-export the world**:

```ts
// packages/core/index.ts
export * from "./user";
export * from "./order";
export * from "./auth";
// ... 80 more lines
```

Barrels feel DRY ("import from one place!") but duplicate the export
graph in a second file that must be kept in sync, break tree-shaking,
and produce cycles. The "one place" is a fiction — the symbols are
still defined in their modules. Import from the source.

## Violations and remedies

### Anti-pattern: magic numbers duplicated

```ts
// apps/api/src/handlers/auth.ts
if (username.length > 64) throw new ApiError("TooLong");

// packages/domain/src/user.ts
if (request.name.length > 64) throw new DomainError("Invalid");

// apps/admin/src/forms.ts
const validate = (s: string) => s.length <= 64;
```

If the limit changes, three places must be edited and someone will
miss the third.

### Idiomatic fix: single source of truth in a domain package

```ts
// packages/domain/src/limits.ts
export const MAX_USERNAME_LEN = 64;
```

```ts
// everywhere else
import { MAX_USERNAME_LEN } from "@org/domain/limits";
if (username.length > MAX_USERNAME_LEN) { /* ... */ }
```

### Anti-pattern: parallel domain type and wire DTO

```ts
// packages/domain/src/user.ts
export interface User {
  id: string;
  email: string;
  name: string;
  createdAt: Date;
}

// apps/api/src/dto/user.ts
export interface UserDto {
  id: string;
  email: string;
  name: string;
  created_at: string;  // wire shape — drifts!
}
```

The same shape appears twice. Adding a `phone` field requires editing
both and a hand-written mapper.

### Idiomatic fix: one Zod schema, two `z.infer`s

```ts
// packages/contracts/src/user.ts
import { z } from "zod";

export const UserWire = z.object({
  id: z.string().uuid(),
  email: z.string().email(),
  name: z.string(),
  created_at: z.string().datetime(),
});

export const User = UserWire.transform((w) => ({
  id: w.id,
  email: w.email,
  name: w.name,
  createdAt: new Date(w.created_at),
}));

export type UserWire = z.input<typeof User>;
export type User    = z.output<typeof User>;
```

Adding a field means: one edit, one schema, two types follow.

### Anti-pattern: `unknown` parsing duplicated at every API boundary

```ts
// every handler
const body = await req.json();
if (typeof body !== "object" || body === null) throw new BadRequest();
if (typeof (body as any).email !== "string") throw new BadRequest();
if (typeof (body as any).name !== "string") throw new BadRequest();
// ...
```

Each route reinvents validation; rules drift.

### Idiomatic fix: a shared `parseBody` helper around the schema

```ts
export async function parseBody<S extends z.ZodTypeAny>(
  req: Request,
  schema: S,
): Promise<z.infer<S>> {
  const json = await req.json().catch(() => {
    throw new ApiError("InvalidJson", 400);
  });
  const result = schema.safeParse(json);
  if (!result.success) throw new ApiError("InvalidBody", 400, result.error);
  return result.data;
}

// use site
const body = await parseBody(req, CreateOrderRequest);
```

One place encodes "how this codebase turns `unknown` into a typed
value at the wire boundary".

### Anti-pattern: duplicated `try/catch` around `fetch`

```ts
async function getUser(id: string) {
  try {
    const r = await fetch(`/api/users/${id}`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return await r.json();
  } catch (e) { logger.error(e); throw e; }
}
async function getOrder(id: string) {
  try {
    const r = await fetch(`/api/orders/${id}`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return await r.json();
  } catch (e) { logger.error(e); throw e; }
}
```

The HTTP-error-handling decision lives in every caller.

### Idiomatic fix: one typed client, schemas at the edge

```ts
export async function apiGet<S extends z.ZodTypeAny>(
  path: string,
  schema: S,
): Promise<z.infer<S>> {
  const r = await fetch(path);
  if (!r.ok) throw new HttpError(r.status, path);
  return schema.parse(await r.json());
}

const getUser  = (id: string) => apiGet(`/api/users/${id}`,  User);
const getOrder = (id: string) => apiGet(`/api/orders/${id}`, Order);
```

### Anti-pattern: parallel validation in API handler and domain

The handler revalidates everything the domain constructor already
checks. Both lists drift.

**Fix**: validation lives in the domain (or in the Zod schema the
domain consumes). The handler translates errors only — it performs
no business validation of its own. A new rule is added in one place.

### Anti-pattern: sync and async copies of the same function

```ts
function loadConfigSync(path: string): Config { /* fs.readFileSync, parse, validate */ }
async function loadConfig(path: string): Promise<Config> { /* fs.promises.readFile, parse, validate */ }
```

The parse-and-validate logic is duplicated. When the schema gains a
field, one copy gets it; the other silently lags.

**Fix**: extract the pure `parseConfig(text: string): Config` and let
each I/O wrapper call it.

```ts
function parseConfig(text: string): Config { return Config.parse(JSON.parse(text)); }
export const loadConfigSync = (p: string) => parseConfig(fs.readFileSync(p, "utf8"));
export const loadConfig     = async (p: string) =>
  parseConfig(await fs.promises.readFile(p, "utf8"));
```

### Anti-pattern: React component prop drilling

```tsx
<App theme={t}><Page theme={t}><Sidebar theme={t}><Nav theme={t}/></Sidebar></Page></App>
```

The fact "the theme is `t`" is repeated at every layer. Adding a
second piece of cross-cutting state (locale, auth) multiplies the
problem.

### Idiomatic fix: React context for genuinely cross-cutting state

```tsx
const ThemeContext = createContext<Theme | null>(null);
export const useTheme = () => {
  const t = useContext(ThemeContext);
  if (!t) throw new Error("ThemeProvider missing");
  return t;
};
```

Use context for things that are **the same knowledge at every level**
(theme, current user, feature flags). Do not reach for context to
DRY-up a single parent→grandchild prop pass; that is Abramov's WET
warning — the duplication is two lines, the abstraction costs a
provider, a hook, a test fixture, and a re-render footgun.

### Anti-pattern: copy-pasted React hooks

```tsx
function UserPage() {
  const [user, setUser]   = useState<User|null>(null);
  const [load, setLoad]   = useState(true);
  const [err,  setErr]    = useState<Error|null>(null);
  useEffect(() => { fetch("/api/me").then(r=>r.json()).then(setUser).catch(setErr).finally(()=>setLoad(false)); }, []);
  // ...
}
function OrderPage() { /* identical 5-line block */ }
```

The async-fetch state-machine is the same knowledge in both. Extract.

### Idiomatic fix: a single `useQuery` hook (or use TanStack Query)

```tsx
function useApi<S extends z.ZodTypeAny>(path: string, schema: S) {
  const [state, setState] = useState<{ data?: z.infer<S>; error?: Error; loading: boolean }>({ loading: true });
  useEffect(() => {
    let cancelled = false;
    apiGet(path, schema)
      .then((data) => !cancelled && setState({ data, loading: false }))
      .catch((error) => !cancelled && setState({ error, loading: false }));
    return () => { cancelled = true; };
  }, [path]);
  return state;
}
```

### Anti-pattern: copy-pasted code that ISN'T DRY

```ts
const calculateTaxUs = (amount: Money) => amount * 0.07;
const calculateTaxEu = (amount: Money) => amount * 0.21;
const calculateTaxUk = (amount: Money) => amount * 0.20;
```

It would be tempting to extract `calculateTax(rate, amount)`. Should
you?

**No** — for two reasons:

1. The three tax rates are not the same knowledge. They are
   independent regulations. If the EU rate changes, the US rate is
   unaffected.
2. The functions communicate intent. `calculateTaxUs(amount)` reads
   better at the call site than `calculateTax(0.07, amount)`.

When VAT rates split by region into 27 individual values that vary
together (per EU directive), THEN extract. The Rule of Three applies.
Resist the urge. Three lookups in a table is fine. (See Sandi Metz,
"The Wrong Abstraction", and Abramov, "The WET Codebase".)

## DRY at the package / monorepo level

Cross-package DRY shows up as:

- A constant duplicated in multiple `package.json`s (e.g. `version`).
  Fix: workspace tooling (`pnpm`/`yarn` workspace protocols, changesets).
- A type duplicated across packages because both need "the same"
  interface. Fix: one defining package, the others depend on it. (Or
  *don't fix* if the two interfaces happen to look alike but mean
  different things.)
- A dependency version pinned in multiple `package.json`s. Fix:
  `pnpm.overrides` / `resolutions` / a single root `package.json`.
- TS config repeated. Fix: a base `tsconfig.base.json` extended by
  each package.

## How code-ranker detects DRY violations

DRY is the hardest principle to detect automatically — knowledge
duplication does not have a graph signature. Code Ranker can flag
*candidates*:

| Signal | DRY interpretation |
|---|---|
| Identical function names across multiple modules (`validate`, `parse`, `format`) | Possible knowledge duplication. Requires name-overlap analysis. |
| Exported constants with identical *values* across multiple packages | Strong DRY-violation candidate. Requires AST inspection. |
| Multiple Zod schemas with the same field set | Strong candidate for a single shared schema. |
| Repeated regex string literals across files | Textbook DRY violation. |
| Multiple packages with similar `dependencies` sets | Possibly the same domain repeated. |

Code Ranker's static graph cannot tell you whether two functions
*encode the same knowledge* — that requires understanding the
function bodies. A future rule could flag literal duplication and
let the LLM-verification step decide.

## Suggested recommendation template

> **DRY candidate** (low confidence): the constant `64` appears as a
> max-length check in 5 places across the workspace
> (`apps/api/handlers/auth.ts`, `packages/domain/user.ts`,
> `apps/admin/forms.ts`, `packages/email/templates.ts`,
> `packages/shared/limits.ts`). If these are encoding the same
> business rule ("usernames must be ≤ 64 chars"), consolidate to a
> single `@org/domain/limits#MAX_USERNAME_LEN`. If they are
> independent (a column width, an email subject limit, a UI hint),
> keep them separate.
>
> Code Ranker cannot tell which case applies. See *Pragmatic Programmer*
> Topic 9, matklad's "Three Levels of Repetition", and Abramov's
> "WET Codebase" for guidance on the call.

## Related principles

- [KISS](KISS.md) — DRY can violate KISS when premature abstraction
  introduces a more complex shape than the duplication. Especially
  true at the type level in TS.
- [YAGNI](YAGNI.md) — don't DRY for a hypothetical second instance
  that may never appear.
- [SRP](SRP.md) — SRP is the discipline that
  produces *true* DRY by aligning code-units with reasons-to-change.

## References

1. Hunt, A. and Thomas, D. *The Pragmatic Programmer: From Journeyman
   to Master*. Addison-Wesley, 1999 (20th anniv. ed., 2019).
   <https://pragprog.com/titles/tpp20/>
2. matklad. "Three Levels of Repetition". 2024.
   <https://matklad.github.io/2024/02/02/three-levels-of-repetition.html>
3. Abramov, D. "The WET Codebase". 2019.
   <https://overreacted.io/the-wet-codebase/>
4. Metz, S. "The Wrong Abstraction". 2016.
   <https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction>
5. Atwood, J. "DRY: It's About Knowledge". 2014.
   <https://blog.codinghorror.com/dry-not-just-about-code/>
