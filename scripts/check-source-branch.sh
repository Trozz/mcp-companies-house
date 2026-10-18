#!/usr/bin/env bash
# Fails unless a pull request into main comes from develop in this repository.
set -euo pipefail

: "${HEAD_REF:?HEAD_REF is required}"
: "${HEAD_REPO:?HEAD_REPO is required}"
: "${BASE_REPO:?BASE_REPO is required}"

if [[ "$HEAD_REF" == "develop" && "$HEAD_REPO" == "$BASE_REPO" ]]; then
  echo "ok: pull request into main comes from ${BASE_REPO}:develop"
  exit 0
fi

echo "::error::Pull requests into main must come from the develop branch of ${BASE_REPO} (got ${HEAD_REPO}:${HEAD_REF}). Target develop instead."
exit 1
