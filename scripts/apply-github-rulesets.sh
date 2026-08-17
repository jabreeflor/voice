#!/usr/bin/env bash
# Apply repository rulesets from .github/rulesets/*.json
#
#   ./scripts/apply-github-rulesets.sh
#
# Requires gh authenticated as a user with admin on this repo
# (the default Actions token cannot create rulesets).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
RULESET_DIR="$ROOT/.github/rulesets"

OWNER_REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
EXISTING=$(gh api "repos/${OWNER_REPO}/rulesets")

shopt -s nullglob
files=("$RULESET_DIR"/*.json)
if [ ${#files[@]} -eq 0 ]; then
    echo "No ruleset JSON files in $RULESET_DIR" >&2
    exit 1
fi

for file in "${files[@]}"; do
    name=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["name"])' "$file")
    id=$(python3 -c '
import json, sys
existing = json.loads(sys.argv[1])
name = sys.argv[2]
for ruleset in existing:
    if ruleset.get("name") == name:
        print(ruleset["id"])
        break
' "$EXISTING" "$name")

    if [ -n "$id" ]; then
        echo "Updating ruleset '$name' (id $id)"
        gh api --method PUT "repos/${OWNER_REPO}/rulesets/${id}" --input "$file"
    else
        echo "Creating ruleset '$name'"
        gh api --method POST "repos/${OWNER_REPO}/rulesets" --input "$file"
    fi
done

echo "Done. Current rulesets:"
gh api "repos/${OWNER_REPO}/rulesets" --jq '.[] | {id, name, enforcement, target}'
