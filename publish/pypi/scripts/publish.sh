#!/usr/bin/env bash
# Copies binaries from dist/ into py-cli/src/codei/bin/, builds a universal wheel, and uploads to PyPI.
#
# Usage: bash scripts/publish.sh [--dry-run]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PY_CLI_ROOT="$(dirname "$SCRIPT_DIR")"
DIST_DIR="$(dirname "$(dirname "$PY_CLI_ROOT")")/dist"
SRC_BIN="${PY_CLI_ROOT}/src/codei/bin"
DRY_RUN="${1:-}"

# Read version from __init__.py
VERSION=$(python3 -c "
import re
with open('${PY_CLI_ROOT}/src/codei/__init__.py') as f:
    m = re.search(r'__version__\s*=\s*\"([^\"]+)\"', f.read())
    print(m.group(1) if m else '0.0.1')
")

echo "📦 Publishing codei v${VERSION} to PyPI"

# Create bin dir
mkdir -p "$SRC_BIN"

# Copy all binaries from dist/, strip version prefix for consistent naming
BINARIES=(
  "codei-${VERSION}-x86_64-unknown-linux-musl|codei-x86_64-unknown-linux-musl"
  "codei-${VERSION}-aarch64-unknown-linux-musl|codei-aarch64-unknown-linux-musl"
  "codei-${VERSION}-x86_64-apple-darwin|codei-x86_64-apple-darwin"
  "codei-${VERSION}-aarch64-apple-darwin|codei-aarch64-apple-darwin"
  "codei-${VERSION}-x86_64-pc-windows-gnullvm.exe|codei-x86_64-pc-windows-gnullvm.exe"
  "codei-${VERSION}-aarch64-pc-windows-gnullvm.exe|codei-aarch64-pc-windows-gnullvm.exe"
)

FOUND_ANY=false

for entry in "${BINARIES[@]}"; do
  IFS='|' read -r src_name dest_name <<< "$entry"
  src="${DIST_DIR}/${src_name}"
  if [[ -f "$src" ]]; then
    echo "📋 Copying ${src_name} → ${dest_name}..."
    cp "$src" "${SRC_BIN}/${dest_name}"
    chmod +x "${SRC_BIN}/${dest_name}" 2>/dev/null || true
    FOUND_ANY=true
  else
    echo "⚠️  Binary not found: ${src}"
  fi
done

if [[ "$FOUND_ANY" != "true" ]]; then
  echo "❌ No binaries found in dist/. Run release workflow first."
  exit 1
fi

# Update version in __init__.py
sed -i "s/__version__ = \"[^\"]*\"/__version__ = \"${VERSION}\"/" "${PY_CLI_ROOT}/src/codei/__init__.py"

# Build wheel
echo "🔨 Building wheel..."
(cd "$PY_CLI_ROOT" && python3 -m pip install --quiet build 2>/dev/null || true)
(cd "$PY_CLI_ROOT" && python3 -m build --wheel)

# Upload
echo "📤 Uploading to PyPI..."
TWINE_CMD="twine upload"
if [[ -n "$DRY_RUN" ]]; then
  TWINE_CMD="echo [dry-run] twine upload"
fi

eval "$TWINE_CMD ${PY_CLI_ROOT}/dist/codei_cli-${VERSION}-*.whl"

echo "✅ Published to PyPI!"
