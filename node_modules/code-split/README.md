# code-split

Polyglot structural-analysis platform. Extract dependency graphs at module / file / function granularity, visualize coupling as an offline HTML report, and track architectural drift between snapshots.

**Status:** pre-alpha. APIs and output shapes may change without notice. Pin a specific version.

## Install

Pick whichever channel fits your toolchain:

```sh
# universal — installs prebuilt binary
curl -fsSL https://github.com/ffedoroff/code-split/releases/latest/download/code-split-installer.sh | sh
```

```sh
# Rust users
cargo install code-split --version 0.1.0-alpha.5
```

```sh
# Node users
npm install -g code-split
```

```sh
# Python users
pip install code-split
```

```powershell
# Windows
powershell -ExecutionPolicy ByPass -c "irm https://github.com/ffedoroff/code-split/releases/latest/download/code-split-installer.ps1 | iex"
```

All channels ship the same `code-split` binary built from the same source.

## Quick start

```sh
# extract dependency graphs from a workspace (writes modules.json / files.json / functions.json)
code-split analyze --plugin rust ./path/to/project

# generate an offline HTML report
code-split report ./snapshots/latest

# diff two snapshots
code-split diff ./snapshots/before ./snapshots/after
```

Built-in language plugins: `rust`, `python`, `javascript` (also handles TypeScript). Third-party plugins are resolved as `code-split-plugin-<name>` on PATH.

## Documentation

- [PRD](docs/PRD.md) — product requirements
- [DESIGN](docs/DESIGN.md) — technical design
- [Principles corpus](principles/) — Rust / Python / TypeScript principle catalogues used for prompt-based code review

## License

Apache-2.0.
