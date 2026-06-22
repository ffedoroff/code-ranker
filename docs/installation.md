# Installation

All channels ship the same `code-ranker` binary, built from the same Rust source —
a fast, memory-safe, single static-ish executable with **no runtime dependencies**
(no Python, no Node, no JVM, no shared libs to install). One file on PATH, done.

**Supported platforms:** Linux (x86_64, aarch64), macOS (x86_64, aarch64),
Windows (x86_64).

**Package pages:** [crates.io](https://crates.io/crates/code-ranker) · [npm](https://www.npmjs.com/package/code-ranker) · [PyPI](https://pypi.org/project/code-ranker/) · [Docker Hub](https://hub.docker.com/r/fedoroff/code-ranker) · [GHCR](https://github.com/ffedoroff/code-ranker/pkgs/container/code-ranker)

## Which channel?

Use the package manager the project **already** depends on — reuse the toolchain
the user has on PATH instead of introducing a new one:

| Project / environment | Use | Command |
|---|---|---|
| Rust (a `Cargo.toml`) | Cargo | `cargo install code-ranker` |
| Web / JavaScript / TypeScript (a `package.json`) | npm | `npm install -g code-ranker` |
| Python (a `pyproject.toml` / `requirements.txt`) | pip / uv / pipx | `pip install code-ranker` |
| CI / containers | Docker | `docker pull ghcr.io/ffedoroff/code-ranker` |
| None of the above / no package manager | Shell installer | the universal one-liner below |

It is the **same binary** regardless of channel, so the choice is purely about
convenience — pick whatever is already installed.

## Pick a channel

```sh
# universal — shell installer that drops the prebuilt binary on PATH
curl -fsSL https://github.com/ffedoroff/code-ranker/releases/latest/download/code-ranker-installer.sh | sh

# Windows
powershell -ExecutionPolicy ByPass -c "irm https://github.com/ffedoroff/code-ranker/releases/latest/download/code-ranker-installer.ps1 | iex"

# Rust (Cargo)
cargo install code-ranker

# Node (npm)
npm install -g code-ranker

# Python (pip / uv / pipx)
pip install code-ranker

# Docker (Docker Hub)
docker pull fedoroff/code-ranker:latest

# Docker (GHCR — no anonymous rate limits)
docker pull ghcr.io/ffedoroff/code-ranker:latest
```

## Verify

```sh
code-ranker --version
```

Then point it at a project — see the [CLI reference](code-ranker-cli/CLI.md) and the
copy-paste [use cases](code-ranker-cli/USE-CASES.md).
