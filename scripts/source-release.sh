#!/usr/bin/env bash
set -euo pipefail

if ! git rev-parse --show-toplevel >/dev/null 2>&1; then
  echo "source-release.sh must run inside a git repository" >&2
  exit 1
fi

ref="${1:-HEAD}"
output_dir="${2:-dist}"

if [[ "${ref}" == "HEAD" ]]; then
  version="$(git rev-parse --short HEAD)"
else
  version="${ref#refs/tags/}"
fi

repo_name="bat-markets"
archive_root="${repo_name}-${version}"
archive_path="${output_dir}/${archive_root}.tar.gz"
checksum_path="${output_dir}/${archive_root}.sha256"

mkdir -p "${output_dir}"
git archive --format=tar.gz --prefix="${archive_root}/" -o "${archive_path}" "${ref}"

archive_name="$(basename "${archive_path}")"
(
  cd "${output_dir}"
  shasum -a 256 "${archive_name}" > "$(basename "${checksum_path}")"
)

echo "created ${archive_path}"
echo "created ${checksum_path}"
