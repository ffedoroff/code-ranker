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

  # Normalize the committed golden so regeneration is idempotent: (1) anonymize
  # machine-specific absolute paths (never leak a real home dir); (2) freeze the
  # time-/environment-dependent header fields (timestamp, git, per-stage `ms`) to
  # fixed sentinels so re-running on unchanged code produces a byte-identical file
  # — no spurious git churn. The e2e test normalizes all of these away anyway, so
  # freezing them in the golden is purely for a clean diff. The graph itself is
  # already deterministic: the tool emits canonical JSON (alphabetical keys,
  # nodes/edges sorted by id).
  python3 - "$dir/code-split-report.json" "$repo_root" "$HOME" <<'PY'
import sys, json
path, repo_root, home = sys.argv[1], sys.argv[2], sys.argv[3]
text = open(path).read()
text = text.replace(repo_root, "/home/user/code-split").replace(home, "/home/user")
d = json.loads(text)
# Freeze volatile header fields (kept raw in the golden, normalized by the e2e test).
d["generated_at"] = "1970-01-01T00:00:00Z"
if "git" in d:
    # Freeze to the canonical SHAPE the e2e test enforces: all fields present,
    # a 12-char commit (`--short=12`) and `origin`. Values are placeholders — the
    # test normalizes them away and checks the live git block's real shape.
    d["git"] = {
        "branch": "main",
        "commit": "000000000000",
        "dirty_files": 0,
        "origin": "git@example.com:org/repo.git",
    }
for t in d.get("timings", []):
    t["ms"] = 0
# Re-emit with the same canonical shape the tool uses: sorted keys, trailing newline.
open(path, "w").write(json.dumps(d, indent=2, sort_keys=True) + "\n")
PY
  echo "regenerated $dir/code-split-report.json"
done
