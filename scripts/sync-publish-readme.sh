#!/usr/bin/env bash
# Copy the repository README into npm/PyPI publish trees so registries show full docs.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
README="${REPO_ROOT}/README.md"

if [[ ! -f "${README}" ]]; then
  echo "sync-publish-readme: README.md not found at ${README}" >&2
  exit 1
fi

cp "${README}" "${REPO_ROOT}/publish/npm/README.md"
cp "${README}" "${REPO_ROOT}/publish/pypi/README.md"

echo "sync-publish-readme: copied README.md to publish/npm and publish/pypi"
