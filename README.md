# code-split

[![CI](https://github.com/ffedoroff/code-split/actions/workflows/ci.yml/badge.svg)](https://github.com/ffedoroff/code-split/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/ffedoroff/code-split/branch/main/graph/badge.svg)](https://codecov.io/gh/ffedoroff/code-split)
[![dependencies](https://deps.rs/crate/code-split/1.0.0-alpha.4/status.svg)](https://deps.rs/crate/code-split/1.0.0-alpha.4)
[![Crates.io](https://img.shields.io/crates/v/code-split.svg)](https://crates.io/crates/code-split)
[![npm](https://img.shields.io/npm/v/code-split.svg)](https://www.npmjs.com/package/code-split)
[![PyPI](https://img.shields.io/pypi/v/code-split.svg)](https://pypi.org/project/code-split/)
[![License](https://img.shields.io/crates/l/code-split.svg)](./LICENSE)

Structural-analysis tool for **Rust, Python, JavaScript and TypeScript** codebases. Built **AI-agent-friendly first** — finds where a project has structural problems and hands an actionable shortlist to a human or an AI agent for the actual refactor.

**👉 Map your codebase's worst structural problems in 30 seconds — [jump to the Rust quick start](#rust-quick-start) and run it on your repo now.**

**Status:** pre-alpha. APIs and output shapes may change without notice. Pin a specific version.

## Rust quick start

```sh
cargo install code-split --version 1.0.0-alpha.4   # install the CLI
code-split report .                                # make html report in .code-split/ folder
```

`report .` needs no flags: it writes a self-contained HTML report (plus a JSON
snapshot) into `.code-split/`. Open the latest `…-<commit>.html` to explore the
dependency graph, per-file metrics, and the AI prompt generator. Everything
below is detail.

## Offline & private

code-split always runs **entirely on your machine**. It makes **no network calls**, sends **no telemetry or analytics**, and **never uploads your code or analysis results** anywhere. Generated HTML reports are self-contained — no CDN, no external requests, no tracking.

## AI agents friendly

**Hand your codebase to an AI agent and let it fix the worst spot.** code-split is built to feed work straight to an AI coding agent (Claude Code, Cursor, …). Attach the short playbook [docs/ai-skill.md](docs/ai-skill.md) to your agent's context — it teaches the agent which two metrics matter (dependency cycles `ADP`, coupling `HK`) and the exact fix loop (scorecard → snapshot → fix → re-check → before/after report).

Then just ask, e.g.:

- *"Read `https://raw.githubusercontent.com/ffedoroff/code-split/main/docs/ai-skill.md`. Find the worst dependency cycle in this project and propose a refactor that breaks it — show me the plan before changing code."*
- *"Read `https://raw.githubusercontent.com/ffedoroff/code-split/main/docs/ai-skill.md`. Find the most complex / highest-HK file and analyze how to split it; explain what the split buys for me (lower coupling, smaller blast radius). Take a **before report**, apply the split, take an **after report**, and show me the **HTML diff**."*

The agent drives the CLI itself — `ai-skill.md` already spells out the commands and the loop, so no glue is needed.

## What it finds

- **Files that grew too complex and should be split.** Per-file cyclomatic / cognitive / Halstead / MI metrics; flags files above your threshold.
- **Strong coupling between files.** Computes fan-in / fan-out / HK on the file dependency graph; surfaces the files that everything depends on (or that depend on everything). Third-party libraries are tracked separately as depth-1 external nodes (`fan_out_external`), so they never inflate your internal-coupling numbers.
- **Cyclic dependencies.** Detects SCCs in the file graph — including the silent ones the compiler does not catch.
- **Files that are just too big.** Raw LOC, public surface size per file.

The tool **does not refactor for you**. It produces a structured, machine-readable list of problem spots and an offline HTML report a human or an LLM can act on.

## CI integration

Runs as a linter. Configure thresholds in `code-split.toml`; the CLI exits non-zero when the codebase breaches them — so a PR that introduces a new cycle, a file above your cognitive budget, or a file above your LOC limit fails the build.

```sh
code-split check . \
  --threshold file.cognitive=25 --threshold file.loc=800
```

The linter is the `check` command — exits non-zero on any cycle or threshold violation, e.g. a PR that introduces a new file-level cycle or a file above your LOC limit (`mutual` and `chain` cycle checks are on by default). See [docs/CLI.md](docs/code-split-cli/CLI.md) for all flags.

**Add it to your pipeline today** — one `code-split check` step stops new cycles and bloat from ever landing.

## Full CLI

Written in Rust — fast, memory-safe, single static-ish binary with **no runtime dependencies** (no Python, no Node, no JVM, no shared libs to install). One file on PATH, done.

Two commands: `check` (linter — exits non-zero on violations; with `--baseline`, a relative regression gate) and `report` (snapshot JSON + offline HTML; with `--baseline`, a baseline↔current diff). Both accept a directory **or** an existing `.json`/`.html` snapshot as input — analyze once, then run cheap passes over the snapshot. No daemon, no language server, no plugin host required at runtime. Full reference: [docs/CLI.md](docs/code-split-cli/CLI.md).

## HTML report with dynamic diagrams

`code-split report` writes a single self-contained HTML file with:

- An interactive file dependency graph; third-party libraries appear as depth-1 external nodes in a distinct amber colour with dashed edges.
- Dagre-laid-out graph with pan/zoom and live filtering.
- Sortable table per metric; click a node to open its neighbourhood.
- "Prompt generator" panel that copies a ready-to-paste prompt (one for each principle: ADP, SRP, OCP, LSP, ISP, DIP, DRY, KISS, LoD, MISU, CoI, YAGNI; plus *Reduce Complexity*, *Split Components*) — feed the prompt + the selected nodes to your AI agent.

No network, no analytics, no telemetry. Open in any browser, share as a file.

**Live demo — code-split run on its own repo:** [interactive HTML report](https://ffedoroff.github.io/code-split/) · [JSON snapshot](https://ffedoroff.github.io/code-split/report.json) (regenerated on every push to `main`).

## Install

**Package pages:** [crates.io](https://crates.io/crates/code-split) · [npm](https://www.npmjs.com/package/code-split) · [PyPI](https://pypi.org/project/code-split/) · [Docker Hub](https://hub.docker.com/r/fedoroff/code-split) · [GHCR](https://github.com/ffedoroff/code-split/pkgs/container/code-split)

Pick a channel:

```sh
# universal — shell installer that drops the prebuilt binary on PATH
curl -fsSL https://github.com/ffedoroff/code-split/releases/latest/download/code-split-installer.sh | sh

# Windows
powershell -ExecutionPolicy ByPass -c "irm https://github.com/ffedoroff/code-split/releases/latest/download/code-split-installer.ps1 | iex"

# Rust (Cargo)
cargo install code-split --version 1.0.0-alpha.4

# Node (npm)
npm install -g code-split

# Python (pip / uv / pipx)
pip install code-split

# Docker (Docker Hub)
docker pull fedoroff/code-split:1.0.0-alpha.4

# Docker (GHCR — no anonymous rate limits)
docker pull ghcr.io/ffedoroff/code-split:1.0.0-alpha.4
```

All channels ship the same `code-split` binary built from the same Rust source. Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64).

## Quick start

```sh
# lint a project — non-zero exit on violations (CI linter)
code-split check ./path/to/project

# analyze and write a snapshot JSON + offline HTML report
code-split report
# → .code-split/{ts}-{git-hash-3}.json + .code-split/{ts}-{git-hash-3}.html
#   (override paths via --output.<fmt>.path or [output.<fmt>] in code-split.toml)

# before / after refactor comparison: an HTML diff against a baseline snapshot
code-split report . --baseline .code-split/before.json
```

Built-in plugins: `rust` (cargo + syn), `python`, `javascript` (also handles TypeScript) — all compiled into the single binary, nothing to install.

## Documentation

- [CLI](docs/code-split-cli/CLI.md) — commands, flags, and examples
- [Rule reference](docs/code-split-cli/ERRORS.md) — rule ids grouped by concern (`CYC`/`CPX`/`CPL`/`SIZ`), per-file thresholds (`file`), what each flags, and how to fix it
- [Config](docs/code-split-cli/config.md) — `code-split.toml` schema
- [AI agent skill](docs/ai-skill.md) — a short playbook to attach to an AI agent's context (the ADP/HK fix loop)
- [PRD](docs/PRD.md) — product requirements
- [DESIGN](docs/DESIGN.md) — technical design
- [Principles corpus](principles/) — Rust / Python / TypeScript principle catalogues used by the prompt generator

## Try it now

```sh
cargo install code-split --version 1.0.0-alpha.4 && code-split report . && open .code-split/
```

One command on any Rust project — you'll have an interactive structural map and an AI-ready shortlist in seconds. ⭐ the repo if it helps.

## License

Apache-2.0.
