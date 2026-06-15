# SARIF v2.1.0 — Developer Reference

> Condensed reference for the **Static Analysis Results Interchange Format (SARIF) v2.1.0**.
> Source: OASIS Standard — <https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html>
> This is a working summary for implementers, not the normative text. When in doubt, consult the spec.

## 1. What SARIF is

SARIF is a standardized, JSON-based format for reporting the results of static analysis
tools. It was designed to "comprehensively capture the range of data produced by commonly
used static analysis tools" and serves two roles:

- a **direct output format** that tools emit natively, and
- an **interchange format** for converting tool-specific outputs into a uniform representation.

It supports diverse artifact types (source code and binaries) and rich metadata about
findings: their locations, severity, remediation guidance, execution context, and history.

A SARIF file **SHALL** be a serialization of the SARIF object model into JSON and **SHALL**
be encoded in UTF-8.

## 2. Top-level structure — `sarifLog`

The root object of every SARIF file:

| Property | Type | Notes |
|---|---|---|
| `version` | string | Must be `"2.1.0"`. |
| `$schema` | string (URI) | URI of the SARIF JSON Schema. |
| `runs` | array of `run` | One entry per tool invocation. |
| `inlineExternalProperties` | array | Optional. Externalized properties embedded in the log. |
| `properties` | object | Optional property bag. |

```json
{
  "version": "2.1.0",
  "$schema": "https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json",
  "runs": []
}
```

## 3. Object model

### `run`

A single invocation of an analysis tool on a set of artifacts.

| Property | Type | Notes |
|---|---|---|
| `tool` | `tool` | **Required.** Identifies the analysis tool. |
| `results` | array of `result` | Detected conditions. |
| `artifacts` | array of `artifact` | Files/artifacts analyzed. |
| `invocations` | array of `invocation` | Tool execution details. |
| `logicalLocations` | array of `logicalLocation` | Programmatic constructs identified. |
| `graphs` | array | Graph structures for advanced flows. |
| `taxonomies` | array of `toolComponent` | Custom classification systems (e.g. CWE). |
| `translations` | array | Localized rule/message strings. |
| `columnKind` | string | `"utf16CodeUnits"` or `"unicodeCodePoints"` — how columns are counted. |
| `newlineSequences` | array of string | Line-ending conventions used. |
| `defaultSourceLanguage` | string | Primary language analyzed. |
| `originalUriBaseIds` | object | Maps base-URI symbols to actual locations. |
| `redactionTokens` | array | Placeholders for redacted sensitive data. |

### `tool`

| Property | Type | Notes |
|---|---|---|
| `driver` | `toolComponent` | **Required.** The main tool executable. |
| `extensions` | array of `toolComponent` | Plugins / rule packs. |

### `toolComponent`

| Property | Type | Notes |
|---|---|---|
| `name` | string | **Required**, localizable. Tool/component identifier. |
| `version` | string | Tool version. |
| `semanticVersion` | string | SemVer format. |
| `guid` | string | Stable unique identifier. |
| `rules` | array of `reportingDescriptor` | Rule definitions. |
| `notifications` | array of `reportingDescriptor` | Tool-notification descriptors. |
| `globalMessageStrings` | object | Shared message templates. |
| `language` | string | Localization language, e.g. `"en-US"`. |
| `shortDescription` / `fullDescription` | `multiformatMessageString` | Documentation. |
| `downloadUri` / `informationUri` | string | Tool links. |
| `organization` | string (localizable) | Vendor/creator. |
| `supportedTaxonomies` | array | References to supported taxonomies. |

### `result`

A detected condition.

| Property | Type | Notes |
|---|---|---|
| `message` | `message` | **Required.** Describes the finding. |
| `ruleId` | string | Symbolic id of the triggered rule. |
| `ruleIndex` | integer | Index into the tool component's `rules` array. |
| `rule` | `reportingDescriptorReference` | Pointer to rule metadata. |
| `kind` | string | `notApplicable`, `pass`, `open`, `review`, `informational`, `fail`. |
| `level` | string | `none`, `note`, `warning`, `error`. Defaults to `warning`. |
| `locations` | array of `location` | Where the finding occurs. |
| `codeFlows` | array of `codeFlow` | Execution paths. |
| `fingerprints` | object | Stable identifiers for cross-run matching. |
| `partialFingerprints` | object | Variant fingerprints for flexible matching. |
| `baselineState` | string | `new`, `unchanged`, `updated`, `absent`. |
| `suppressions` | array of `suppression` | Why a result is ignored. |
| `taxa` | array | References to applicable taxonomies. |
| `rank` | number | Confidence/importance, `0.0`–`100.0`. |
| `guid` / `correlationGuid` | string | Unique / cross-run identifiers. |
| `fixes` | array of `fix` | Proposed remediations. |
| `workItemUris` | array | Links to issue trackers. |
| `attachments` | array | Supporting files. |
| `provenance` | object | Detection history, first/last occurrence. |

### `message`

| Property | Type | Notes |
|---|---|---|
| `text` | string | Plain-text content. |
| `markdown` | string | GitHub-Flavored Markdown version. |
| `id` | string | Index into a rule's `messageStrings` for templated messages. |
| `arguments` | array of string | Values substituted into a templated message. |

Messages support placeholders (`{0}`, `{1}`, …) and embedded links to artifact locations.

### `multiformatMessageString`

| Property | Type | Notes |
|---|---|---|
| `text` | string | Plain-text (required if `markdown` absent). |
| `markdown` | string | Formatted version. |

Used for rule descriptions, help text, and similar content.

### `location`

| Property | Type | Notes |
|---|---|---|
| `id` | integer | Unique within the result. |
| `physicalLocation` | `physicalLocation` | File/offset reference. |
| `logicalLocations` | array of `logicalLocation` | Programmatic constructs. |
| `message` | `message` | Context-specific message. |
| `annotations` | array of `region` | Highlighted sub-regions. |
| `relationships` | array | Links to other locations. |

### `physicalLocation`

| Property | Type | Notes |
|---|---|---|
| `artifactLocation` | `artifactLocation` | **Required.** Identifies the file. |
| `region` | `region` | Exact location within the file. |
| `contextRegion` | `region` | Surrounding context. |
| `address` | object | Memory address for binary analysis. |

### `artifactLocation`

| Property | Type | Notes |
|---|---|---|
| `uri` | string | File path or URL (may be relative). |
| `uriBaseId` | string | Symbol resolving relative URIs to absolute. |
| `index` | integer | Index into `run.artifacts`. |
| `description` | `message` | Explanatory message. |

### `region`

Text-region properties:

| Property | Type | Notes |
|---|---|---|
| `startLine` / `endLine` | integer (1-based) | Line numbers. |
| `startColumn` / `endColumn` | integer (1-based) | Column positions. |
| `charOffset` / `charLength` | integer | Character-level positioning. |
| `snippet` | `artifactContent` | Quoted text of the region. |

Binary-region properties: `byteOffset`, `byteLength`. Also: `message`, `sourceLanguage`.

### `logicalLocation`

References a programmatic construct without specifying its containing artifact.

| Property | Type | Notes |
|---|---|---|
| `name` | string | Most specific component (e.g. method name). |
| `fullyQualifiedName` | string | Complete hierarchical identifier (e.g. `N.C.f(void)`). |
| `decoratedName` | string | Language-decorated name (signatures, mangling). |
| `kind` | string | `function`, `variable`, `module`, `namespace`, `type`, `parameter`, … |
| `parentIndex` | integer | Index of the containing logical location (nesting). |

### `artifact`

| Property | Type | Notes |
|---|---|---|
| `location` | `artifactLocation` | The artifact's location. |
| `parentIndex` | integer | Containing artifact (nesting). |
| `contents` | `artifactContent` | Text/binary payload. |
| `encoding` | string | Character encoding, e.g. `"utf-8"`. |
| `sourceLanguage` | string | `c`, `cpp`, `csharp`, `javascript`, … |
| `hashes` | object | Cryptographic hashes (`sha-256`, `sha-1`, …). |
| `mimeType` | string | Content type. |
| `roles` | array of string | `analysisTarget`, `attachment`, `source`, `test`, `added`, `modified`, `deleted`, `renamed`, `driver`, `library`, … |
| `lastModifiedTimeUtc` | string | ISO 8601 timestamp. |
| `description` | `message` | Documentation. |

### `reportingDescriptor` (rule / notification metadata)

| Property | Type | Notes |
|---|---|---|
| `id` | string | **Required.** Unique rule id (e.g. `CA2001`). |
| `name` | string (localizable) | Human-readable name. |
| `shortDescription` / `fullDescription` | `multiformatMessageString` | Documentation. |
| `messageStrings` | object | Templates for parameterized messages. |
| `helpUri` | string | Link to documentation. |
| `help` | `multiformatMessageString` | Embedded help text. |
| `defaultConfiguration` | `reportingConfiguration` | Default severity, etc. |
| `relationships` | array | Links to other descriptors/taxa. |
| `guid` | string | Stable identifier. |
| `deprecatedIds` / `deprecatedGuids` / `deprecatedNames` | arrays | Legacy values. |
| `taxa` | array | Taxonomy classifications (e.g. CWE). |

### `reportingConfiguration`

| Property | Type | Notes |
|---|---|---|
| `enabled` | boolean | Rule active. Defaults to `true`. |
| `level` | string | Severity override. |
| `rank` | number | Confidence/importance `0.0`–`100.0`. |
| `parameters` | object | Rule-specific configuration. |

### `codeFlow` → `threadFlow` → `threadFlowLocation`

`codeFlow`:

| Property | Type | Notes |
|---|---|---|
| `message` | `message` | Optional flow description. |
| `threadFlows` | array of `threadFlow` | **Required.** One per execution thread. |

`threadFlow`:

| Property | Type | Notes |
|---|---|---|
| `id` | string | Thread identifier. |
| `message` | `message` | Optional context. |
| `locations` | array of `threadFlowLocation` | **Required.** Ordered execution sequence. |
| `initialState` / `immutableState` | object | Variable states. |

`threadFlowLocation`:

| Property | Type | Notes |
|---|---|---|
| `location` | `location` | Optional. |
| `stack` | `stack` | Optional call frames. |
| `module` | string | Module/DLL name. |
| `kinds` | array of string | `call`, `return`, `branch`, `enter`, `exit`, `taint`, `acquire`, `release`, … |
| `state` | object | Variable values at this point. |
| `nestingLevel` | integer | Call-stack depth. |
| `executionOrder` | integer | Sequence in the thread. |
| `executionTimeUtc` | string | When the step executed. |
| `importance` | string | `important` or `unimportant`. |

### `stackFrame`

| Property | Type | Notes |
|---|---|---|
| `location` | `location` | Optional. |
| `module` | string | Module name. |
| `threadId` | integer | OS thread id. |
| `parameters` | array of string | Function arguments. |

### `fix` → `artifactChange` → `replacement`

`fix`:

| Property | Type | Notes |
|---|---|---|
| `description` | `message` | Optional. |
| `artifactChanges` | array of `artifactChange` | **Required.** |

`artifactChange`:

| Property | Type | Notes |
|---|---|---|
| `artifactLocation` | `artifactLocation` | **Required.** Target file. |
| `replacements` | array of `replacement` | **Required.** |

`replacement`:

| Property | Type | Notes |
|---|---|---|
| `deletedRegion` | `region` | Region to remove. |
| `insertedContent` | `artifactContent` | Content to insert. |

### `invocation`

| Property | Type | Notes |
|---|---|---|
| `commandLine` | string (redactable) | Command invoked. |
| `arguments` | array (redactable) | Parsed arguments. |
| `responseFiles` | array | Config files loaded. |
| `ruleConfigurationOverrides` / `notificationConfigurationOverrides` | arrays | Runtime settings. |
| `startTimeUtc` / `endTimeUtc` | string | Execution timestamps. |
| `exitCode` / `exitCodeDescription` | integer / string | Process exit. |
| `machine` / `account` / `processId` | string / string / integer | Host/user/process. |
| `executableLocation` | `artifactLocation` | Tool binary. |
| `workingDirectory` | `artifactLocation` | Working folder. |
| `environmentVariables` | object | Environment at execution. |
| `toolExecutionNotifications` / `toolConfigurationNotifications` | arrays of `notification` | Issues during run. |
| `stdin` / `stdout` / `stderr` | string | Standard streams. |
| `executionSuccessful` | boolean | Whether the tool completed normally. |

### `notification`

| Property | Type | Notes |
|---|---|---|
| `descriptor` | `reportingDescriptorReference` | Optional. |
| `message` | `message` | **Required.** |
| `level` | string | `none`, `note`, `warning`, `error`. |
| `locations` | array of `location` | Where it applies. |
| `threadId` | integer | If thread-specific. |
| `timeUtc` | string | When it occurred. |
| `exception` | `exception` | Error details. |

### `suppression`

| Property | Type | Notes |
|---|---|---|
| `kind` | string | **Required.** `inSource` or `external`. |
| `status` | string | `accepted`, `underReview`, `rejected`. |
| `justification` | string | For `inSource`: `notApplicable`, `notProductionCode`, `intentional`, `mitigated`. |
| `location` | `location` | In-source suppression point. |
| `guid` | string | Unique suppression id. |

### `fingerprints` & `partialFingerprints`

Stable identifiers for matching results across runs. Both are objects of versioned
hierarchical strings, e.g. `"myHash/v1": "abc123"`.

- `fingerprints` — identify the complete identity of a result.
- `partialFingerprints` — match on a subset of properties (more tolerant to small changes).

A key without a `v{number}` suffix is considered older than versioned variants.

### `properties` (property bag)

Every SARIF object **MAY** include a `properties` object for custom key-value data:

- names are hierarchical camelCase strings; values may be any JSON type;
- enables tool-specific extensions without changing the spec;
- a `tags` array inside the bag provides concise categorization, e.g. `"tags": ["security"]`.

## 4. `kind` vs `level` semantics

These are orthogonal:

- **`kind`** — the *type* of the result: did the check fail (`fail`), pass (`pass`), is it
  informational, open, under review, or not applicable.
- **`level`** — the *severity* when `kind` is `fail`: `none` → `note` → `warning` → `error`.

`level` defaults to `warning`. When `kind` is anything other than `fail`, `level` should be
`none` (the finding is not a failure, so severity does not apply).

## 5. How rules are referenced

Three complementary mechanisms tie a `result` to its rule metadata
(`reportingDescriptor`):

1. **`ruleId`** (string) — symbolic id such as `"CA2001"`, stored directly on the result.
2. **`ruleIndex`** (integer) — zero-based index into the relevant tool component's `rules`
   array (usually `tool.driver.rules`).
3. **`rule`** (`reportingDescriptorReference`) — combines `id` / `index` / `guid` plus an
   optional `toolComponent` reference, allowing the rule to come from an extension/plugin.

A `reportingDescriptorReference` may resolve a descriptor by `id`, `index`, `guid`, or the
owning `toolComponent`.

## 6. Examples

### Minimal valid SARIF

```json
{
  "version": "2.1.0",
  "$schema": "https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json",
  "runs": [
    {
      "tool": {
        "driver": {
          "name": "MyAnalyzer"
        }
      },
      "results": []
    }
  ]
}
```

### Fuller example — a result with a location and a rule

```json
{
  "version": "2.1.0",
  "$schema": "https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json",
  "runs": [
    {
      "tool": {
        "driver": {
          "name": "SecurityAnalyzer",
          "version": "1.0",
          "rules": [
            {
              "id": "SEC001",
              "name": "SqlInjection",
              "shortDescription": { "text": "Detects potential SQL injection vulnerabilities." },
              "help": { "text": "Use parameterized queries to prevent injection attacks." },
              "defaultConfiguration": { "level": "error" }
            }
          ]
        }
      },
      "artifacts": [
        {
          "location": { "uri": "src/database.js" },
          "sourceLanguage": "javascript"
        }
      ],
      "results": [
        {
          "ruleId": "SEC001",
          "ruleIndex": 0,
          "kind": "fail",
          "level": "error",
          "message": {
            "text": "User input is concatenated into SQL query without parameterization."
          },
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": { "uri": "src/database.js", "index": 0 },
                "region": {
                  "startLine": 42,
                  "startColumn": 15,
                  "endLine": 42,
                  "endColumn": 58,
                  "snippet": { "text": "query(\"SELECT * FROM users WHERE id=\" + userId)" }
                }
              }
            }
          ],
          "rank": 8.5,
          "baselineState": "new"
        }
      ]
    }
  ]
}
```
