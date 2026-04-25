#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

runtime_crates=(
  bat-markets-core
  bat-markets-binance
  bat-markets-bybit
  bat-markets
)

workspace_crates=(
  "${runtime_crates[@]}"
  bat-markets-testing
)

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

fail() {
  echo "release verification failed: $*" >&2
  exit 1
}

manifest_for_crate() {
  case "$1" in
    bat-markets) echo "crates/bat-markets/Cargo.toml" ;;
    bat-markets-binance) echo "crates/bat-markets-binance/Cargo.toml" ;;
    bat-markets-bybit) echo "crates/bat-markets-bybit/Cargo.toml" ;;
    bat-markets-core) echo "crates/bat-markets-core/Cargo.toml" ;;
    bat-markets-testing) echo "crates/bat-markets-testing/Cargo.toml" ;;
    *) fail "unknown crate '$1'" ;;
  esac
}

lock_version_for_crate() {
  local crate="$1"
  awk -v crate="${crate}" '
    BEGIN { FS = "\"" }
    function flush() {
      if (name == crate) {
        print version
        found = 1
        exit
      }
    }
    /^\[\[package\]\]/ {
      flush()
      name = ""
      version = ""
      next
    }
    /^name = / { name = $2 }
    /^version = / { version = $2 }
    END {
      if (!found && name == crate) {
        print version
      }
    }
  ' Cargo.lock
}

[[ -n "${workspace_version}" ]] || fail "missing [workspace.package] version"

release_ref="${BAT_MARKETS_RELEASE_TAG:-${GITHUB_REF_NAME:-}}"
if [[ -n "${release_ref}" && "${release_ref}" == v* && "${release_ref}" != "v${workspace_version}" ]]; then
  fail "release ref ${release_ref} does not match workspace version ${workspace_version}"
fi

cargo metadata --locked --no-deps --format-version 1 >/dev/null

grep -q "## ${workspace_version} -" CHANGELOG.md \
  || fail "CHANGELOG.md is missing an entry for ${workspace_version}"

for crate in "${workspace_crates[@]}"; do
  manifest="$(manifest_for_crate "${crate}")"
  grep -q '^version\.workspace = true$' "${manifest}" \
    || fail "${manifest} must inherit version.workspace = true"

  locked_version="$(lock_version_for_crate "${crate}")"
  [[ "${locked_version}" == "${workspace_version}" ]] \
    || fail "Cargo.lock has ${crate} ${locked_version:-<missing>}, expected ${workspace_version}"
done

for manifest in $(find crates -mindepth 2 -maxdepth 2 -name Cargo.toml | sort); do
  while IFS= read -r line; do
    dep_name="$(sed -E 's/^([a-z0-9-]+) = .*/\1/' <<<"${line}")"
    dep_version="$(sed -E 's/.*version = "([^"]+)".*/\1/' <<<"${line}")"
    [[ "${dep_version}" == "${workspace_version}" ]] \
      || fail "${manifest} depends on ${dep_name} ${dep_version}, expected ${workspace_version}"
  done < <(grep -E '^bat-markets(-[a-z]+)? = \{[^}]*version = "' "${manifest}" || true)
done

echo "release verification passed for v${workspace_version}"
