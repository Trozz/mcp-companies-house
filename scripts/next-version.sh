#!/usr/bin/env bash
# Computes the next version of the mcp-companies-house server.
# See README "Releases" for the rules. Writes key=value lines to stdout and $GITHUB_OUTPUT.
set -euo pipefail

bump_rank() {
  case "$1" in
    major) echo 3 ;;
    minor) echo 2 ;;
    patch) echo 1 ;;
    *) echo 0 ;;
  esac
}

highest_bump() {
  local best="" word
  while IFS= read -r word; do
    word="$(printf '%s' "$word" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]')"
    if (( $(bump_rank "$word") > $(bump_rank "$best") )); then
      best="$word"
    fi
  done
  printf '%s' "$best"
}

bump_from_labels() {
  printf '%s\n' "$1" | tr ',' '\n' | highest_bump
}

bump_from_title() {
  { printf '%s' "$1" | grep -oiE '\[(major|minor|patch)\]' || true; } | tr -d '[]' | highest_bump
}

resolve_bump() {
  local from_labels from_title
  from_labels="$(bump_from_labels "$1")"
  if [[ -n "$from_labels" ]]; then
    printf '%s' "$from_labels"
    return
  fi
  from_title="$(bump_from_title "$2")"
  if [[ -n "$from_title" ]]; then
    printf '%s' "$from_title"
    return
  fi
  printf 'patch'
}

apply_bump() {
  local major minor patch
  IFS=. read -r major minor patch <<<"$1"
  case "$2" in
    major) printf '%s.0.0' "$((major + 1))" ;;
    minor) printf '%s.%s.0' "$major" "$((minor + 1))" ;;
    *) printf '%s.%s.%s' "$major" "$minor" "$((patch + 1))" ;;
  esac
}

latest_stable_tag() {
  { git tag --list "$1[0-9]*" --sort=-v:refname | grep -E "^$1[0-9]+\.[0-9]+\.[0-9]+$" || true; } | head -n1
}

last_release_tag() {
  if [[ "$2" == "main" ]]; then
    latest_stable_tag "$1"
    return
  fi
  git describe --tags --abbrev=0 --match "$1[0-9]*" HEAD 2>/dev/null || true
}

has_relevant_changes() {
  local since="$1"
  if [[ -z "$since" ]]; then
    return 0
  fi
  git diff --name-only "$since" HEAD \
    | grep -qE "^(crates/|Cargo\.lock$|Cargo\.toml$|Dockerfile$)"
}

lookup_pr() {
  if [[ -n "${PR_TITLE+x}" || -n "${PR_LABELS+x}" ]]; then
    return
  fi
  local filter='.[0] // {} | {title: (.title // ""), labels: ([.labels[]?.name] | join(","))}'
  local json
  case "$BRANCH" in
    main)
      json="$(gh api "repos/${GITHUB_REPOSITORY}/commits/${GITHUB_SHA}/pulls" --jq "$filter")"
      ;;
    develop)
      json="$(gh pr list --repo "$GITHUB_REPOSITORY" --base main --head develop --state open --json title,labels --jq "$filter")"
      ;;
  esac
  PR_TITLE="$(jq -r '.title' <<<"$json")"
  PR_LABELS="$(jq -r '.labels' <<<"$json")"
}

main() {
  : "${BRANCH:?BRANCH must be develop or main}"
  local prefix="${TAG_PREFIX:-v}"
  if [[ "$BRANCH" != "develop" && "$BRANCH" != "main" ]]; then
    echo "BRANCH must be develop or main, got '$BRANCH'" >&2
    return 1
  fi
  lookup_pr

  local bump stable base version tag last release range
  bump="$(resolve_bump "${PR_LABELS:-}" "${PR_TITLE:-}")"
  stable="$(latest_stable_tag "$prefix")"
  base="0.0.0"
  if [[ -n "$stable" ]]; then
    base="${stable#"${prefix}"}"
  fi
  version="$(apply_bump "$base" "$bump")"
  if [[ "$BRANCH" == "develop" ]]; then
    version="${version}-dev.${RUN_NUMBER:?RUN_NUMBER is required on develop}"
  fi
  tag="${prefix}${version}"

  last="$(last_release_tag "$prefix" "$BRANCH")"
  release=false
  if has_relevant_changes "$last"; then
    release=true
  fi
  if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
    release=false
  fi

  range=""
  if [[ -n "$stable" ]]; then
    range="${stable}..HEAD"
  fi

  {
    echo "version=${version}"
    echo "tag=${tag}"
    echo "release=${release}"
    echo "bump=${bump}"
    echo "previous=${stable}"
    echo "range=${range}"
  } | tee -a "${GITHUB_OUTPUT:-/dev/null}"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
