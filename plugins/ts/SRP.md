# SRP — Single Responsibility Principle (in TypeScript)

**TL;DR**: A module, class, or function should have one reason to change.
In a TypeScript codebase, this most often means: a single file should not
export twenty unrelated helpers; a single React component should not
fetch, transform, validate, and render; a single backend service class
should not own CRUD plus auth plus billing plus notifications; a barrel
(`index.ts`) should not re-export from every leaf in the tree.

## Canonical sources

- Robert C. Martin, "The Principles of OOD" (originally in *More C++ Gems*,
  1996; later in *Clean Architecture*, 2017): "A class should have only one
  reason to change." Source: <https://blog.cleancoder.com/uncle-bob/2014/05/08/SingleReponsibilityPrinciple.html>
- Robert C. Martin, *Clean Architecture*, Ch. 7: refines the principle to
  "A module should be responsible to one, and only one, actor."
- David L. Parnas, "On the Criteria To Be Used in Decomposing Systems into
  Modules", *CACM* 15(12), 1972 — the original "secrets" argument: each
  module hides one design decision, so it has one reason to change.
- Kent C. Dodds, "When to break up a component into multiple components"
  (2019): "One component, one job." Splitting along responsibility, not
  along line-count, is what makes React trees navigable.
  <https://kentcdodds.com/blog/when-to-break-up-a-component-into-multiple-components>
- Mark Seemann, "The Single Responsibility Principle" (2011):
  <https://blog.ploeh.dk/2011/03/22/SOLIDinIntroductoryProgramming/>

## The principle

Martin's later formulation is the most useful: **a module is responsible
to one actor**. An "actor" is any stakeholder whose needs drive
changes — a regulatory body, a product owner, a downstream team, a
design system, an analytics vendor. When a module serves two actors,
changes requested by one are forced through review by the other, and
the module accumulates conflicting pressures.

The popular short form — "one reason to change" — is sometimes
misread as "one function per file" or "one component per file no matter
how trivial". That is not the principle. The unit of responsibility is
**change pressure**: if two pieces of code consistently change for the
same reason, they belong together; if they change for different reasons,
they belong apart.

In TypeScript, the unit of "responsibility" most naturally maps to a
package (a workspace/Nx/Turborepo package), then to a folder, then to a
file, then to an exported function/class/component. Bare functions
inside a file are usually too fine-grained — splitting a 50-line
function in two does not change which actor causes it to evolve.

## Why it matters

A module shared by multiple actors becomes a coordination chokepoint:

- Every PR must pass review by every actor's team.
- Tests proliferate because each actor's changes can break the others'.
- Refactoring is "expensive" because so many callers depend on the
  module's exact shape — and `import` statements in TS are everywhere,
  often through barrels that obscure the true dependency.
- Stack traces and commit history become hard to read: a single file
  with eight reasons to change has eight times the commit churn.
- `tsc` rebuild times rise because every change invalidates a node
  that half the codebase imports through.

SRP is the principle that keeps a TypeScript repo **navigable**. When
you violate it, the symptom is not a runtime bug — it is the gradual
realization that nobody on the team feels comfortable touching
`utils.ts`, `helpers.ts`, or `lib/index.ts`.

## In TypeScript

The TypeScript ecosystem applies SRP at multiple granularities:

| Unit | What "one responsibility" means |
|---|---|
| Monorepo | The whole product / library family |
| Package | One bounded context (an "actor's worth" of code) |
| Folder | One coherent concept inside a package |
| File/module | One implementation concern (e.g. one component, one hook, one service) |
| Class/component/hook | One thing it represents |
| Function | One mental step the caller performs |

The most common SRP violations in real TypeScript codebases:

1. **`utils.ts` / `helpers.ts` god files** — a single module that
   accumulates `formatDate`, `clamp`, `slugify`, `assertNever`,
   `parseJwt`, `deepEqual`, `retry`, … exported as 20+ unrelated
   functions. Imported by everyone; touched in every other PR.
2. **Barrel re-export soup** — `lib/index.ts` that does
   `export * from "./a"; export * from "./b"; …` across 30 leaves,
   creating a single node with fan-in approaching the whole repo and
   defeating tree-shaking and `tsc --build` incrementality.
3. **God React components** — a single `.tsx` mixing data fetching
   (`useQuery`), local state, derived state, validation, business
   rules, layout, and presentation, with a 200-line `useEffect`
   coordinating all of it.
4. **God services / fat controllers** — a Nest/Express
   `UserService` (or a Next.js `app/api/users/route.ts`) doing
   persistence, validation, RBAC, audit, email, and metrics in one
   class or one route handler.
5. **Static-method namespaces** — `class StringUtils { static a(); static b(); … }`
   used as a "poor man's module". The class has no instances; it is
   just a god module with extra ceremony.

## Violations and remedies

### Anti-pattern: god `UserService` class

```ts
// Bad: one class, many actors.
export class UserService {
  constructor(
    private db: Db,
    private cache: Redis,
    private audit: AuditSink,
    private metrics: Metrics,
    private mailer: Mailer,
  ) {}

  async createUser(...): Promise<User> { /* ... */ }
  async deactivateUser(...): Promise<void> { /* ... */ }
  async recordLogin(...): Promise<void> { /* ... */ }
  async exportForGdpr(...): Promise<Buffer> { /* ... */ }
  async sendWelcomeEmail(...): Promise<void> { /* ... */ }
  async rotatePassword(...): Promise<void> { /* ... */ }
  async assignRole(...): Promise<void> { /* ... */ }
  async auditAdminChange(...): Promise<void> { /* ... */ }
  // ... 30 more methods
}
```

Reasons to change: GDPR compliance (legal), email templates (marketing),
auth flow (security), RBAC (product), audit retention (ops). Five
different actors, one class.

### Idiomatic fix: split by actor

```ts
export class UserRepository {
  constructor(private db: Db) {}
  create(...): Promise<User> { /* ... */ }
  deactivate(id: UserId): Promise<void> { /* ... */ }
}

export class UserAuthService {
  constructor(private repo: UserRepository, private hasher: Argon2) {}
  rotatePassword(...): Promise<void> { /* ... */ }
  recordLogin(...): Promise<void> { /* ... */ }
}

export class UserComplianceService {
  constructor(private repo: UserRepository, private audit: AuditSink) {}
  exportForGdpr(...): Promise<Buffer> { /* ... */ }
}

export class UserNotifier {
  constructor(private mailer: Mailer) {}
  welcome(...): Promise<void> { /* ... */ }
}
```

Each class now has one actor. Legal changes touch only
`UserComplianceService`; marketing touches only `UserNotifier`.

### Anti-pattern: god `utils.ts`

```ts
// src/lib/utils.ts — 1800 LOC, 47 exports
export function formatDate(d: Date, fmt: string) { /* ... */ }
export function clamp(n: number, lo: number, hi: number) { /* ... */ }
export function slugify(s: string) { /* ... */ }
export function parseJwt(token: string) { /* ... */ }
export function deepEqual(a: unknown, b: unknown) { /* ... */ }
export function retry<T>(fn: () => Promise<T>, opts: RetryOpts) { /* ... */ }
export function assertNever(x: never): never { throw new Error(); }
// ... 40 more
```

Every feature team imports this; every PR changes it. There is no
coherent actor — `parseJwt` belongs to security, `formatDate` to i18n,
`retry` to infra resilience.

### Idiomatic fix: one concern per file, no barrel umbrella

```
src/lib/
├── date/format.ts        // i18n actor
├── number/clamp.ts       // math actor
├── string/slugify.ts     // content actor
├── auth/jwt.ts           // security actor
├── async/retry.ts        // resilience actor
└── types/assert-never.ts // type-system actor
```

Import the leaf you need:

```ts
import { formatDate } from "@/lib/date/format";
import { retry } from "@/lib/async/retry";
```

Do **not** add a `src/lib/index.ts` that re-exports the whole tree —
that recreates the god module under a different name and defeats
tree-shaking.

### Anti-pattern: barrel re-export soup

```ts
// src/index.ts
export * from "./users";
export * from "./billing";
export * from "./notifications";
export * from "./auth";
export * from "./reports";
// ... 25 more
```

This file has fan-in ≈ the entire repo and fan-out ≈ every feature
folder. Touching any leaf invalidates the barrel's declaration file;
`tsc --build` rebuilds every consumer; tree-shakers cannot prove
unused exports dead. It is one module serving every actor.

### Idiomatic fix: deep imports, narrow barrels

Keep barrels only at intentional package boundaries (the public API
of a published package). Inside a package, import the leaf module
directly. A barrel that re-exports more than one cohesive concern is
already an SRP violation.

### Anti-pattern: god React component

```tsx
// Bad: one component, many jobs.
export function UserDashboard({ userId }: { userId: string }) {
  const [tab, setTab] = useState<"overview" | "billing" | "activity">("overview");
  const [filter, setFilter] = useState("");
  const { data: user } = useQuery(["user", userId], () => api.getUser(userId));
  const { data: invoices } = useQuery(["inv", userId], () => api.getInvoices(userId));
  const { data: events } = useQuery(["ev", userId], () => api.getEvents(userId));

  useEffect(() => {
    // 80 lines: validate state, sync to URL, refetch on focus,
    // record analytics, derive permission flags, prefetch next tab...
  }, [user, invoices, events, tab, filter]);

  if (!user) return <Spinner />;
  // 300 lines of JSX mixing layout, business rules, and presentational details
  return <div>{/* ... */}</div>;
}
```

Reasons to change: API shape (backend), analytics taxonomy (data),
URL scheme (product), permissions (security), visual design (design
system). At least five actors in one `.tsx` file.

### Idiomatic fix: one component, one job (Kent C. Dodds)

```tsx
// Data fetching lives in hooks.
function useUserDashboardData(userId: string) {
  const user = useUser(userId);
  const invoices = useInvoices(userId);
  const events = useEvents(userId);
  return { user, invoices, events };
}

// Cross-cutting effects live in their own hooks.
function useDashboardAnalytics(tab: TabId) { /* ... */ }
function useDashboardUrlSync(tab: TabId, filter: string) { /* ... */ }

// Each tab is its own component, owning its own layout.
function OverviewTab({ user }: { user: User }) { /* ... */ }
function BillingTab({ invoices }: { invoices: Invoice[] }) { /* ... */ }
function ActivityTab({ events }: { events: Event[] }) { /* ... */ }

// The shell only composes.
export function UserDashboard({ userId }: { userId: string }) {
  const [tab, setTab] = useState<TabId>("overview");
  const { user, invoices, events } = useUserDashboardData(userId);
  useDashboardAnalytics(tab);
  useDashboardUrlSync(tab, "");

  if (!user) return <Spinner />;
  return (
    <DashboardShell tab={tab} onTabChange={setTab}>
      {tab === "overview" && <OverviewTab user={user} />}
      {tab === "billing" && <BillingTab invoices={invoices ?? []} />}
      {tab === "activity" && <ActivityTab events={events ?? []} />}
    </DashboardShell>
  );
}
```

The shell has one reason to change: the layout/composition of the
dashboard. Each tab and hook has its own actor.

### Anti-pattern: Next.js `app/` route doing everything

```ts
// app/api/orders/route.ts
export async function POST(req: Request) {
  const body = await req.json();
  // 1. Validate
  if (!body.items?.length) return Response.json({ error: "..." }, { status: 400 });
  // 2. Auth
  const session = await getSession(req);
  if (!session) return new Response("Unauthorized", { status: 401 });
  // 3. Persist
  const order = await db.order.create({ data: { ... } });
  // 4. Charge
  const stripeResp = await stripe.charges.create({ ... });
  // 5. Notify
  await mailer.send({ ... });
  // 6. Audit
  await audit.record({ ... });
  return Response.json(order);
}
```

Five reasons to change in one route handler.

### Idiomatic fix: route delegates; collaborators own concerns

```ts
// app/api/orders/route.ts
export async function POST(req: Request) {
  const session = await requireSession(req);
  const input = parseOrderInput(await req.json());          // validation actor
  const order = await orderRepo.create(session.userId, input); // persistence
  await paymentGateway.charge(order);                       // payments
  await orderNotifier.confirm(order);                       // notifications
  await audit.recordOrder(order);                           // compliance
  return Response.json(order);
}
```

The handler has one job: sequence the steps. Each collaborator lives
in its own file with its own actor.

### Anti-pattern: static-method namespace class

```ts
export class DateUtils {
  static format(d: Date, fmt: string) { /* ... */ }
  static parseIso(s: string) { /* ... */ }
  static diffDays(a: Date, b: Date) { /* ... */ }
  static isWeekend(d: Date) { /* ... */ }
}
```

There are no instances; this is a module wearing a `class` keyword.
It also blocks tree-shaking — bundlers cannot drop unused statics.

### Idiomatic fix: bare named exports in a focused module

```ts
// src/lib/date/format.ts
export function formatDate(d: Date, fmt: string) { /* ... */ }
export function parseIso(s: string) { /* ... */ }
export function diffDays(a: Date, b: Date) { /* ... */ }
export function isWeekend(d: Date) { /* ... */ }
```

Tree-shakeable, no ceremony. If the file grows beyond one concern,
split by sub-actor (`format.ts`, `arithmetic.ts`, `predicates.ts`).

## SRP at the package level

The same principle scales up. A package is "responsible to one actor"
when its changelog is intelligible: every released version answers a
single question — "what changed for `X`?". When a package has releases
labelled "add OpenAPI client, fix retry backoff, bump zod, add OAuth
PKCE, format errors", it is serving five actors and is a refactoring
candidate.

A monorepo-friendly layout:

```
repo/
├── packages/
│   ├── db/             // storage actor
│   ├── auth/           // security actor
│   ├── http-client/    // transport actor
│   └── errors/         // error vocabulary actor
└── apps/
    ├── web/            // user-facing product actor
    ├── admin/          // ops-facing product actor
    └── worker/         // background-jobs actor
```

Each package has one actor; cross-package dependencies are explicit in
`package.json` rather than smuggled through a top-level barrel.

## How code-ranker detects SRP violations

Code Ranker cannot read actors directly, but the graph signatures of an
SRP violation are unambiguous:

| Signal | SRP interpretation |
|---|---|
| Module with high fan-in × fan-out (god-module-coupling rule) | Module serves multiple unrelated siblings |
| File LOC and export-count breaching mega-file thresholds | Single file accumulating multiple concerns |
| Barrel re-export module entangled in an SCC (prelude-sibling-cycle rule) | Barrel acts as both a facade and a participant in unrelated subsystems |
| Public export with very high fan-in (high-fan-in-public-api rule) | Single API surface used by many unrelated actors — every change is a coordination event |
| Component file > N LOC with > M hooks (god-component rule) | React component mixing data, logic, and presentation |

Cross-references in code-ranker's catalog:

- `god-module-coupling` directly maps to "module-serving-many-actors"
- `mega-file` maps to "file-with-too-many-reasons-to-change"
- `prelude-sibling-cycle` maps to "barrel-conflated-with-participation"
- `god-component` maps to "one-component-many-jobs"

## Suggested recommendation template

When code-ranker detects a candidate SRP violation, the Finding should:

1. Quote Martin's "one reason to change" / "one actor" (and, for
   components, Kent C. Dodds' "one component, one job").
2. Pin the violation to the offending node (file, component, or class).
3. Ask the user to enumerate the *actors* whose changes touch this
   module in the last N months (informally — this is qualitative).
4. Suggest a split along those actor lines.
5. Cite Martin's clean-coder post and (for UI) Dodds' essay.

Example body:

> **SRP violation candidate**: `src/lib/utils.ts` has 47 exports and
> fan-in 112. SRP (Martin 1996; Parnas 1972) prescribes one reason
> to change per module. Identify the actors driving recent commits to
> this file; if more than two are visible, split by concern into leaf
> files (`date/`, `string/`, `auth/`, `async/`) and import the leaf
> directly rather than through a barrel.

## Related principles

- [Open/Closed Principle](OCP.md) — what to do once SRP
  has been applied: keep each unit closed to modification.
- [Interface Segregation Principle](ISP.md) —
  same idea applied to interface/type surface, not module surface.
- [DRY](DRY.md) — distinct: SRP is about *why* code changes; DRY is
  about *whether* knowledge is duplicated.
- [High Cohesion / Low Coupling](CoI.md) —
  SRP is the cohesion lever; CoI is the coupling lever.

## References

1. Martin, R. C. "The Single Responsibility Principle". Clean Coder
   Blog, 2014. <https://blog.cleancoder.com/uncle-bob/2014/05/08/SingleReponsibilityPrinciple.html>
2. Martin, R. C. *Clean Architecture: A Craftsman's Guide to Software
   Structure and Design*. Prentice Hall, 2017. Ch. 7 — "SRP: The
   Single Responsibility Principle".
3. Parnas, D. L. "On the Criteria To Be Used in Decomposing Systems
   into Modules". *Communications of the ACM* 15(12), 1972.
4. Dodds, K. C. "When to break up a component into multiple
   components". 2019.
   <https://kentcdodds.com/blog/when-to-break-up-a-component-into-multiple-components>
5. Seemann, M. "The Single Responsibility Principle". Ploeh blog, 2011.
   <https://blog.ploeh.dk/2011/03/22/SOLIDinIntroductoryProgramming/>
