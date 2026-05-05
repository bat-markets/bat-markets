#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

dry_run=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      dry_run=true
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
  shift
done

./scripts/verify-release.sh

workspace_version="$(
  awk '
    /^\[workspace.package\]/ { in_workspace_package = 1; next }
    /^\[/ { in_workspace_package = 0 }
    in_workspace_package && /^version = / {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' Cargo.toml
)"

if [[ -z "${workspace_version}" ]]; then
  echo "could not resolve [workspace.package] version from Cargo.toml" >&2
  exit 1
fi

release_tag="${BAT_MARKETS_RELEASE_TAG:-${GITHUB_REF_NAME:-}}"
if [[ -n "${release_tag}" && "${release_tag}" == v* && "${release_tag}" != "v${workspace_version}" ]]; then
  echo "release tag ${release_tag} does not match workspace version ${workspace_version}" >&2
  exit 1
fi

require_clean_worktree() {
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "publishing requires a clean git worktree" >&2
    git status --short >&2
    exit 1
  fi
}

if [[ "${dry_run}" == "true" ]]; then
  cargo package --workspace --exclude bat-markets-testing
  echo "dry run complete for v${workspace_version}"
  exit 0
fi

require_clean_worktree

if [[ "${release_tag}" != v* ]]; then
  echo "publishing requires a v-prefixed release tag; set BAT_MARKETS_RELEASE_TAG or run from a v* GitHub ref" >&2
  exit 1
fi

if [[ -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "CARGO_REGISTRY_TOKEN is required for crates.io publishing" >&2
  exit 1
fi

publish_order=(
  bat-markets-core
  bat-markets-binance
  bat-markets-bybit
  bat-markets-mexc
  bat-markets
)

registry_has_version() {
  local crate="$1"
  cargo info "${crate}@${workspace_version}" --registry crates-io >/dev/null 2>&1
}

wait_for_registry() {
  local crate="$1"

  for _ in $(seq 1 60); do
    if registry_has_version "${crate}"; then
      echo "${crate} v${workspace_version} is visible in crates.io"
      return 0
    fi
    sleep 10
  done

  echo "${crate} v${workspace_version} did not become visible in crates.io in time" >&2
  exit 1
}

for crate in "${publish_order[@]}"; do
  if registry_has_version "${crate}"; then
    echo "${crate} v${workspace_version} is already published; skipping"
    continue
  fi

  cargo publish -p "${crate}"
  wait_for_registry "${crate}"
done

echo "published bat-markets workspace v${workspace_version}"
