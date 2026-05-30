# code-split

[![CI](https://github.com/ffedoroff/code-split/actions/workflows/ci.yml/badge.svg)](https://github.com/ffedoroff/code-split/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/ffedoroff/code-split/branch/main/graph/badge.svg)](https://codecov.io/gh/ffedoroff/code-split)
[![Crates.io](https://img.shields.io/crates/v/code-split.svg)](https://crates.io/crates/code-split)
[![npm](https://img.shields.io/npm/v/code-split.svg)](https://www.npmjs.com/package/code-split)
[![PyPI](https://img.shields.io/pypi/v/code-split.svg)](https://pypi.org/project/code-split/)
[![License](https://img.shields.io/crates/l/code-split.svg)](./LICENSE)

Structural-analysis tool for **Rust, Python, JavaScript and TypeScript** codebases. Built **AI-agent-friendly first** — finds where a project has structural problems and hands an actionable shortlist to a human or an AI agent for the actual refactor.

**Status:** pre-alpha. APIs and output shapes may change without notice. Pin a specific version.

## Offline & private

code-split always runs **entirely on your machine**. It makes **no network calls**, sends **no telemetry or analytics**, and **never uploads your code or analysis results** anywhere. Generated HTML reports are self-contained — no CDN, no external requests, no tracking.

## What it finds

- **Components that grew too complex and should be split.** Per-function and per-module cyclomatic / cognitive / Halstead / MI metrics; flags entities above your threshold.
- **Strong coupling between components.** Computes fan-in / fan-out / HK on the call and module graphs; surfaces the modules that everything depends on (or that depend on everything).
- **Cyclic dependencies.** Detects SCCs at module, file and function level — including the silent ones the compiler does not catch.
- **Files that are just too big.** Raw LOC, public surface size, item / method counts per file.

The tool **does not refactor for you**. It produces a structured, machine-readable list of problem spots and an offline HTML report a human or an LLM can act on.

## CI integration

Runs as a linter. Configure thresholds in `code-split.toml`; the CLI exits non-zero when the codebase breaches them — so a PR that introduces a new cycle, a function above your cognitive budget, or a file above your LOC limit fails the build.

```sh
code-split check . \
  --threshold node.cognitive=25 --threshold node.loc=800
```

The linter is the `check` command — exits non-zero on any cycle or threshold violation (`mutual` and `chain` cycle checks are on by default). See [docs/CLI.md](docs/CLI.md) for all flags.

## Full CLI

Written in Rust — fast, memory-safe, single static-ish binary with **no runtime dependencies** (no Python, no Node, no JVM, no shared libs to install). One file on PATH, done.

Three commands: `check` (linter — exits non-zero on violations), `report` (snapshot JSON + offline HTML, with optional in-run before/after diff via `--before`), `diff` (HTML or JSON diff between two snapshots, for CI artifacts). No daemon, no language server, no plugin host required at runtime. Full reference: [docs/CLI.md](docs/CLI.md).

## HTML report with dynamic diagrams

`code-split report` writes a single self-contained HTML file with:

- Three interactive levels: modules, files, functions.
- Dagre-laid-out graph with pan/zoom and live filtering.
- Sortable tables per metric; click a node to open its neighbourhood.
- "Prompt generator" panel that copies a ready-to-paste prompt (one for each principle: ADP, SRP, OCP, LSP, ISP, DIP, DRY, KISS, LoD, MISU, CoI, YAGNI; plus *Reduce Complexity*, *Split Components*) — feed the prompt + the selected nodes to your AI agent.

No network, no analytics, no telemetry. Open in any browser, share as a file.

## Install

**Package pages:** [crates.io](https://crates.io/crates/code-split) · [npm](https://www.npmjs.com/package/code-split) · [PyPI](https://pypi.org/project/code-split/) · [Docker Hub](https://hub.docker.com/r/fedoroff/code-split) · [GHCR](https://github.com/ffedoroff/code-split/pkgs/container/code-split)

Pick a channel:

```sh
# universal — shell installer that drops the prebuilt binary on PATH
curl -fsSL https://github.com/ffedoroff/code-split/releases/latest/download/code-split-installer.sh | sh

# Windows
powershell -ExecutionPolicy ByPass -c "irm https://github.com/ffedoroff/code-split/releases/latest/download/code-split-installer.ps1 | iex"

# Rust (Cargo)
cargo install code-split --version 0.1.0-alpha.11

# Node (npm)
npm install -g code-split

# Python (pip / uv / pipx)
pip install code-split

# Docker (Docker Hub)
docker pull fedoroff/code-split:0.1.0-alpha.11

# Docker (GHCR — no anonymous rate limits)
docker pull ghcr.io/ffedoroff/code-split:0.1.0-alpha.11
```

All channels ship the same `code-split` binary built from the same Rust source. Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64).

## Quick start

```sh
# lint a project — non-zero exit on violations (CI linter)
code-split check ./path/to/project

# analyze and write a snapshot JSON + offline HTML report
code-split report ./path/to/project
# → .code-split/{project-dir}-{ts}.json + .code-split/index.html

# before / after refactor comparison of two snapshots
code-split diff --before .code-split/before.json --after .code-split/after.json
```

Built-in plugins: `rust` (cargo + syn + rust-analyzer), `python`, `javascript` (also handles TypeScript) — all compiled into the single binary, nothing to install.

## Documentation

- [PRD](docs/PRD.md) — product requirements
- [DESIGN](docs/DESIGN.md) — technical design
- [Principles corpus](principles/) — Rust / Python / TypeScript principle catalogues used by the prompt generator

## License

Apache-2.0.
