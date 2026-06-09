# CI

How continuous integration runs for `code-ranker`.

## Workflow

`.github/workflows/ci.yml` runs on every push to `main` and on every pull
request. Three jobs run in parallel:

| Job | Steps | Gate |
|---|---|---|
| **Test & lint** | `cargo fmt --all --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace` | Fails the build on any formatting drift, clippy warning, or failing test. |
| **Coverage** | `cargo llvm-cov --workspace --lcov` → upload to Codecov | Measures line/region coverage and reports it to Codecov; does not fail the build. |
| **Security audit** | `cargo audit` against the [RustSec](https://rustsec.org) advisory DB | Fails the build on a known vulnerability in any locked dependency. Informational advisories (unmaintained / unsound) are reported but do **not** fail — only actual vulnerabilities gate. |

Toolchain is `stable` (`dtolnay/rust-toolchain`); builds are cached with
`Swatinem/rust-cache`. Coverage uses `cargo-llvm-cov` (installed via
`taiki-e/install-action`).

## Reproduce locally

The `test` job mirrors the `Makefile`:

```sh
make all        # build + test + lint
# or individually:
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Coverage, the same way CI generates it:

```sh
cargo llvm-cov --workspace --lcov --output-path lcov.info   # for Codecov
cargo llvm-cov --workspace --summary-only                   # quick % in the terminal
```

Security audit (`cargo install cargo-audit` first):

```sh
cargo audit
```

## Codecov

The coverage job uploads `lcov.info` to [Codecov](https://codecov.io). It needs
a `CODECOV_TOKEN` repository secret (Settings → Secrets and variables →
Actions). Upload is non-blocking (`fail_ci_if_error: false`), so CI stays green
even if the token is missing — only the coverage badge goes stale.

## Other workflows

CI is separate from the release plumbing — `release.yml`, `crates-io.yml`,
`pypi.yml`, and `docker.yml` only run on `v*` tags (see the `Makefile` release
section), not on push or PR.
