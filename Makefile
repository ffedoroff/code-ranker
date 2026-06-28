.PHONY: all build test e2e clippy lint-md machete machete-fix lint check self-check coverage diff-coverage fmt fmt-check clean bump tag release publish

all: build test lint self-check coverage

build:
	cargo build --workspace

test:
	cargo test --workspace

# End-to-end fixture tests: run the built binary on each samples/<lang> project
# and compare its JSON report against the committed golden. Refresh goldens with
# `bash samples/regen.sh` after an intentional change.
e2e:
	cargo test -p code-ranker --test e2e

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

fmt:
	cargo fmt --all

# Mirrors CI's `Format` step — fails on unformatted code instead of rewriting it.
fmt-check:
	cargo fmt --all --check

lint-md:
	lychee --offline --no-progress --exclude-path plugins/_overlay 'docs/**/*.md' 'contrib/**/*.md' 'plugins/**/*.md' 'AGENTS.md' 'CLAUDE.md'
	npx --yes markdownlint-cli2

# Unused-dependency check (fast, stable toolchain). FAILS the build on any unused
# crate dependency — keeping Cargo.toml honest. To resolve: drop the dep, run
# `make machete-fix` to remove it automatically, or — for a genuine false
# positive (a dep used only via macro/re-export) — whitelist it under
# `[package.metadata.cargo-machete] ignored = [...]` in that crate's Cargo.toml.
# Detect-only on purpose: `make all` never silently rewrites Cargo.toml.
machete:
	cargo machete --version >/dev/null 2>&1 || cargo install cargo-machete
	cargo machete

machete-fix:
	cargo machete --version >/dev/null 2>&1 || cargo install cargo-machete
	cargo machete --fix

lint: fmt-check clippy machete lint-md

# Dogfood: run code-ranker's own gate on this repo (the thresholds + cycle rules
# in code-ranker.toml). Part of `make all`, so a regression here fails the build.
self-check:
	cargo run -q -p code-ranker -- check .

# Coverage floor: fail if workspace line coverage drops below 90%. Part of
# `make all`. Needs cargo-llvm-cov (`cargo install cargo-llvm-cov`).
coverage:
	cargo llvm-cov --workspace --summary-only --fail-under-lines 90

# Surgical companion to `coverage`: list the lines this branch ADDED/CHANGED vs
# the target branch that no test covers (a review aid — add a test where it's
# organic, skip the genuinely hard-to-test ones). Not in `make all`.
diff-coverage:
	python3 .claude/scripts/diff-coverage.py

check: build test clippy lint-md

clean:
	cargo clean

# --- Release plumbing --------------------------------------------------------
#
#   make bump VERSION=0.1.0-alpha.12      # edit configs + cargo build (no commit)
#                                         # → review `git diff`, commit yourself
#   make release                          # push current branch + tag v$VERSION
#                                         # (version read from Cargo.toml; refuses if dirty)
#
# bump replaces the current version everywhere it appears:
#   - Cargo.toml (workspace.package.version + 4 workspace.dependencies versions)
#   - README.md install snippets
# pyproject.toml uses `dynamic = ["version"]` — maturin pulls from Cargo.toml
# automatically, no manual sync needed.
#
# It does NOT commit — review the diff, edit if needed, then `git commit`.
#
# Two-phase release:
#   make release  (phase 1) pushes branch + tag v$VERSION -> triggers Verify ONLY
#                 (full checks + packaging dry-runs + token preflight, NO publish).
#   make publish  (phase 2) is the single Release button: after Verify is green it
#                 dispatches publish.yml to release everywhere
#                 (crates.io / PyPI / Docker / GitHub Release + npm).
#
# GitHub-ONLY prerelease (an alpha for testing — GitHub Release + binaries, and
# NOTHING on any registry). The registries are SEPARATE workflows that run only
# from publish.yml, so do NOT use `make publish` here — dispatch release.yml
# directly and it publishes a GitHub Release + binaries and nothing else. The one
# registry job baked into release.yml is npm; set `publish-prereleases = false` in
# dist-workspace.toml and it (and any other publish-job) is SKIPPED for a
# prerelease tag. Cut it from a THROWAWAY branch so the alpha version bump never
# lands on main:
#     git checkout -b release/vX.Y.Z-alpha
#     # in dist-workspace.toml: publish-prereleases = false
#     make bump VERSION=X.Y.Z-alpha && git commit -am 'release vX.Y.Z-alpha'
#     git push -u origin release/vX.Y.Z-alpha
#     git tag -a vX.Y.Z-alpha -m vX.Y.Z-alpha && git push origin vX.Y.Z-alpha
#     gh workflow run release.yml --ref release/vX.Y.Z-alpha -f tag=vX.Y.Z-alpha
# Verify goes RED on the PyPI job for a non-PEP-440 suffix like `-pre-alpha` —
# expected and harmless: a direct release.yml dispatch is not gated by Verify and
# never runs the PyPI/crates/Docker workflows. Delete the branch + tag when done.

bump:
	@if [ -z "$(VERSION)" ]; then echo "usage: make bump VERSION=0.1.0-alpha.12"; exit 1; fi
	@CURRENT=$$(grep -E '^version = "' Cargo.toml | head -1 | sed -E 's/version = "(.*)"/\1/'); \
	  if [ "$$CURRENT" = "$(VERSION)" ]; then \
	    echo "already at $(VERSION) — nothing to bump"; exit 1; \
	  fi; \
	  echo "bumping $$CURRENT -> $(VERSION)"; \
	  LC_ALL=C sed -i '' "s/$$CURRENT/$(VERSION)/g" Cargo.toml README.md; \
	  RE=$$(printf '%s' "$$CURRENT" | sed 's/[.]/\\./g'); \
	  for f in $$(grep -rlF "$$CURRENT" docs AGENTS.md CLAUDE.md plugins 2>/dev/null || true); do \
	    LC_ALL=C sed -i '' -E "/code-ranker|--version/ s/$$RE/$(VERSION)/g" "$$f" && echo "  ✓ fixed doc version refs in $$f"; \
	  done
	cargo build --workspace
	@echo
	@echo "  remaining stale version mentions in docs (auto-fix only touches code-ranker/--version lines):"
	@hits=$$(grep -rnE -- '--version[ =]+v?[0-9]+\.[0-9]+\.[0-9]+|[0-9]+\.[0-9]+\.[0-9]+-(alpha|beta|rc)|code-ranker[@:" ]+v?[0-9]+\.[0-9]+\.[0-9]+' docs README.md AGENTS.md CLAUDE.md plugins 2>/dev/null | grep -vF "$(VERSION)" || true); \
	  if [ -n "$$hits" ]; then echo "$$hits" | sed 's/^/      /'; echo "      ^ not at $(VERSION) — review (bare numbers off code-ranker/--version lines are left alone on purpose)"; else echo "      (none — all at $(VERSION))"; fi
	@echo
	@echo "  ✓ bumped to $(VERSION) — review and commit:"
	@echo "      git diff --stat && git add -A && git commit -m 'release v$(VERSION)'"
	@echo "  → then: make release"

release:
	@if ! git diff-index --quiet HEAD --; then \
	  echo "working tree is dirty — commit first"; \
	  git status --short; \
	  exit 1; \
	fi
	@VERSION=$$(grep -E '^version = "' Cargo.toml | head -1 | sed -E 's/version = "(.*)"/\1/'); \
	  BRANCH=$$(git symbolic-ref --short HEAD); \
	  echo "tagging v$$VERSION from branch $$BRANCH (triggers Verify, NO publish)"; \
	  git push origin $$BRANCH; \
	  git tag -a v$$VERSION -m "v$$VERSION"; \
	  git push origin v$$VERSION; \
	  echo; \
	  echo "  ✓ tag v$$VERSION pushed — Verify is running (nothing published yet)"; \
	  echo "  → watch:   gh run list --repo ffedoroff/code-ranker --limit 6"; \
	  echo "  → release: make publish   (only after Verify is green)"

publish:
	@VERSION=$$(grep -E '^version = "' Cargo.toml | head -1 | sed -E 's/version = "(.*)"/\1/'); \
	  echo "dispatching Release for v$$VERSION (crates=$${CRATES:-true} pypi=$${PYPI:-true} docker=$${DOCKER:-true} github_release=$${GITHUB_RELEASE:-true})"; \
	  gh workflow run publish.yml --repo ffedoroff/code-ranker \
	    -f version="$$VERSION" \
	    -f crates="$${CRATES:-true}" -f pypi="$${PYPI:-true}" \
	    -f docker="$${DOCKER:-true}" -f github_release="$${GITHUB_RELEASE:-true}"; \
	  echo "  ✓ dispatched — watch: gh run list --repo ffedoroff/code-ranker --limit 6"
