# Prompt-Generator scaffolding

The language-neutral framing prose the Prompt Generator wraps a principle in,
parsed into `PromptTemplate` by `prompt_template()` and carried in the snapshot so
the CLI `prompt` format and the HTML viewer render the same text from one source.
Each `## <field>` section maps to a `PromptTemplate` field; `## task` is a list
(one entry per bullet, kept verbatim — the leading `- ` is part of the rendered
line). `{id}` in a `task` line is substituted with the active preset id at render
time. This is internal template prose, not a published corpus doc — it lives next
to `builtin.toml`, not under `languages/`.

## intro

I want to apply this to some modules in my system.

## doc_note

Download and read the full principle to understand it in detail. If you cannot download it, **stop the task immediately**.

## task

- Prepare a precise, detailed estimate and a report of where the modules below violate it.
- If you find more serious violations elsewhere during research, mention them in the report too.
- Show a summary of the report in chat.
- If any violation is found, suggest saving the report to a file as a plan for a detailed review, named `.code-ranker/<YYYYMMDD-HHMMSS>-{id}.md`.

## focus

**Focus the research and report primarily on the modules below.**

## cycle_note

This is **one** dependency cycle; every module in it is listed below so the whole loop is visible. Fix one cycle at a time — `--top 2`+ lists several separate cycles at once and obscures how each one connects.
