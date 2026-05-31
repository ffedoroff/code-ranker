#!/usr/bin/env bash
# Regenerate the committed code-split-report.json for every sample fixture.
#
# Each sample is analyzed with ITS OWN code-split.toml (pinned plugin, test
# files kept) so the result does not depend on any repo-level config. The graph
# paths are already relativized to {target} by the tool; the report header
# (generated_at, command, git, versions, absolute paths, timings) stays raw and
# is normalized only at comparison time by the e2e test.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo build -p code-split --quiet
bin="$repo_root/target/debug/code-split"

# cargo metadata for the Rust sample must run offline against the local cache.
export CARGO_NET_OFFLINE=true

for sample in rust python javascript typescript; do
  dir="samples/$sample"
  "$bin" report "$dir" \
    --config "$dir/code-split.toml" \
    --format json \
    --report-path "$dir" \
    --json-name code-split-report.json

  # Anonymize machine-specific absolute paths in the report header so the
  # committed golden never leaks a real home directory. The graph is already
  # relativized to {target}; here we only touch the raw header strings. The
  # e2e test normalizes these fields anyway, so the exact placeholder is cosmetic.
  python3 - "$dir/code-split-report.json" "$repo_root" "$HOME" <<'PY'
import sys
path, repo_root, home = sys.argv[1], sys.argv[2], sys.argv[3]
text = open(path).read()
text = text.replace(repo_root, "/home/user/code-split").replace(home, "/home/user")
open(path, "w").write(text)
PY
  echo "regenerated $dir/code-split-report.json"
done
