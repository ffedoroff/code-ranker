.PHONY: all build test e2e clippy lint-md lint check fmt clean bump tag release

all: build test lint

build:
	cargo build --workspace

test:
	cargo test --workspace

# End-to-end fixture tests: run the built binary on each samples/<lang> project
# and compare its JSON report against the committed golden. Refresh goldens with
# `bash samples/regen.sh` after an intentional change.
e2e:
	cargo test -p code-split --test e2e

clippy:
	cargo clippy --workspace -- -D warnings

fmt:
	cargo fmt --all

lint-md:
	lychee --offline --no-progress 'docs/**/*.md' 'contrib/**/*.md' 'principles/**/*.md' 'AGENTS.md' 'CLAUDE.md'
	npx --yes markdownlint-cli2

lint: clippy lint-md

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
# release reads the current version from Cargo.toml, pushes the current branch
# and creates+pushes annotated tag `v$VERSION`. The tag push triggers all 4
# release workflows (Release / crates.io / PyPI / Docker).

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
	  echo "releasing v$$VERSION from branch $$BRANCH"; \
	  git push origin $$BRANCH; \
	  git tag -a v$$VERSION -m "v$$VERSION"; \
	  git push origin v$$VERSION; \
	  echo; \
	  echo "  ✓ tag v$$VERSION pushed — 4 release workflows starting on GitHub"; \
	  echo "  → watch: gh run list --repo ffedoroff/code-split --limit 5"
