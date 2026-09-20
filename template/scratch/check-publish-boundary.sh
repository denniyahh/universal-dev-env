#!/usr/bin/env bash
set -euo pipefail

ZERO_SHA='0000000000000000000000000000000000000000'
BASE_REF="${GSD_PUBLISH_BASE_REF:-upstream/next}"
violations=()

is_local_only_path() {
  case "$1" in
    .agents|.agents/*|.planning|.planning/*|mise.toml|scratch|scratch/*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

if ! git rev-parse --verify --quiet "refs/remotes/${BASE_REF#refs/remotes/}" >/dev/null; then
  echo "Push blocked: required publish base '$BASE_REF' is not available locally." >&2
  echo "Fetch it before pushing: git fetch upstream next" >&2
  exit 1
fi

while read -r _local_ref local_sha _remote_ref _remote_sha; do
  [ -z "${local_sha:-}" ] && continue
  [ "$local_sha" = "$ZERO_SHA" ] && continue

  while IFS= read -r path; do
    [ -z "$path" ] && continue
    if is_local_only_path "$path"; then
      violations+=("$path")
    fi
  done < <(git diff --name-only --diff-filter=ACMRT "$BASE_REF...$local_sha")
done

if [ "${#violations[@]}" -gt 0 ]; then
  {
    echo "Push blocked: the contribution diff contains personal-only paths."
    echo "Base: $BASE_REF"
    printf '  - %s\n' "${violations[@]}" | sort -u
    echo "Keep these files local and commit only the intended upstream change."
  } >&2
  exit 1
fi
