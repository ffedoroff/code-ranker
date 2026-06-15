.PHONY: all build test e2e clippy lint-md lint check self-check coverage diff-coverage fmt fmt-check clean bump tag release publish

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
	lychee --offline --no-progress 'docs/**/*.md' 'contrib/**/*.md' 'principles/**/*.md' 'AGENTS.md' 'CLAUDE.md'
	npx --yes markdownlint-cli2

lint: fmt-check clippy lint-md

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

bump:
	@if [ -z "$(VERSION)" ]; then echo "usage: make bump VERSION=0.1.0-alpha.12"; exit 1; fi
	@CURRENT=$$(grep -E '^version = "' Cargo.toml | head -1 | sed -E 's/version = "(.*)"/\1/'); \
	  if [ "$$CURRENT" = "$(VERSION)" ]; then \
	    echo "already at $(VERSION) — nothing to bump"; exit 1; \
	  fi; \
	  echo "bumping $$CURRENT -> $(VERSION)"; \
	  LC_ALL=C sed -i '' "s/$$CURRENT/$(VERSION)/g" Cargo.toml README.md
	cargo build --workspace
	@echo
	@echo "  ✓ bumped to $(VERSION) — review and commit:"
	@echo "      git diff --stat && git diff Cargo.toml pyproject.toml README.md"
	@echo "      git add -A && git commit -m 'release v$(VERSION)'"
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
