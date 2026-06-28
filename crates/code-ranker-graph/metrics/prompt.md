# Prompt-Generator scaffolding

The language-neutral framing prose the Prompt Generator wraps a principle in,
parsed into `PromptTemplate` by `prompt_template()` and carried in the snapshot so
the CLI `prompt` format and the HTML viewer render the same text from one source.
Each `## <field>` section maps to a `PromptTemplate` field; `## task` is a list
(one entry per bullet, kept verbatim — the leading `- ` is part of the rendered
line). In a `task` or `doc_note` line, `{id}` is substituted with the active
principle/metric id and `{lang}` with the resolved language at render time (e.g.
`code-ranker docs {lang} {id}` → `code-ranker docs rust HK`). This is internal
template prose, not a published corpus doc — it lives next to `builtin.toml`, not
under `languages/`.

## intro

I want to apply this to some modules in my system.

## doc_note

**First, before reading the source**, run `code-ranker docs {lang} {id}` — it prints the full principle and, for your language, the usual cause of this exact violation and the smallest correct fix, often with a worked example and how to confirm it. Read it first: it normally names the remedy outright, so you apply it instead of re-deriving the mechanism from the code.

## task

- Prepare a precise, detailed estimate and a report of where the modules below violate it.
- If you find more serious violations elsewhere during research, mention them in the report too.
- Show a summary of the report in chat.
- If any violation is found, suggest saving the report to a file as a plan for a detailed review, named `.code-ranker/<YYYYMMDD-HHMMSS>-{id}.md`.

## focus

**Focus the research and report primarily on the modules below.**

## cycle_note

This is **one** dependency cycle; every module in it is listed below so the whole loop is visible. Fix one cycle at a time — `--top 2`+ lists several separate cycles at once and obscures how each one connects.
