#!/usr/bin/env bash
# upload.sh — overwrite the v0.4.54 latest.json asset with the corrected
# manifest (keys = target triples instead of Tauri-1-style short aliases).
#
# Requires: gh (GitHub CLI) authenticated against the e9ab98/NiceSSH repo.
# Usage:    bash dist-update/upload.sh
set -euo pipefail

REPO="e9ab98/NiceSSH"
TAG="v0.4.54"
ASSET_NAME="latest.json"
HERE="$(cd "$(dirname "$0")" && pwd)"
LOCAL_FILE="$HERE/latest.json"

if ! command -v gh >/dev/null 2>&1; then
  echo "::error::gh CLI not installed. Install with: brew install gh" >&2
  exit 1
fi

# Validate the local file is JSON with correct keys before uploading
python3 - <<PY
import json, sys
d = json.load(open("$LOCAL_FILE"))
keys = list(d["platforms"].keys())
expected = {
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
}
got = set(keys)
missing = expected - got
extra   = got - expected
if missing or extra:
    print(f"::error::platform keys mismatch. missing={sorted(missing)} extra={sorted(extra)}", file=sys.stderr)
    sys.exit(1)
print(f"OK: version={d['version']}, platforms={sorted(keys)}")
PY

# Find the existing asset id for the release tag, so we can delete-and-replace
# (gh release upload --clobber does overwrite, but explicit is friendlier)
echo "→ Looking up existing asset id for $TAG/$ASSET_NAME"
ASSET_ID="$(gh release view "$TAG" --repo "$REPO" --json assets \
  --jq ".assets[] | select(.name == \"$ASSET_NAME\") | .id" || true)"

if [ -n "${ASSET_ID:-}" ]; then
  echo "→ Deleting existing asset id=$ASSET_ID"
  gh release delete-asset "$TAG" "$ASSET_ID" --repo "$REPO" --yes
fi

echo "→ Uploading corrected $ASSET_NAME to $TAG"
gh release upload "$TAG" "$LOCAL_FILE" \
  --repo "$REPO" \
  --clobber \
  --label "Auto-generated updater manifest" || {
    echo "::error::upload failed. The release tag may not exist or token lacks write access." >&2
    exit 1
  }

echo "→ Done. Verifying..."
sleep 2
curl -sSL "https://github.com/$REPO/releases/download/$TAG/$ASSET_NAME" \
  | python3 -c "import json,sys; d=json.load(sys.stdin); print('live keys:', sorted(d['platforms'].keys()))"
